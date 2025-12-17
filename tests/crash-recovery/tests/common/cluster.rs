//! Cluster test environment for multi-node Raft testing.
//!
//! This is the canonical test environment for both cluster tests and crash tests.
//! Even single-node crash tests use `ClusterEnv::new(1)` to test with Raft enabled.

use super::environment::TestEnvironment;
use super::verify::{verify_no_orphans_or_gc_pending, verify_no_phantom_objects};
use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// Time to wait for Raft servers to start before initializing cluster
const RAFT_STARTUP_DELAY: Duration = Duration::from_millis(500);

/// Status response from /cluster/status endpoint.
#[derive(Debug, Deserialize)]
pub struct ClusterStatusResponse {
    pub node_id: u64,
    pub state: String,
    pub current_term: u64,
    pub current_leader: Option<u64>,
    pub last_applied_index: Option<u64>,
    pub last_log_index: Option<u64>,
    pub voters: Vec<u64>,
    pub learners: Vec<u64>,
    pub member_count: usize,
}

/// Response from membership operations.
#[derive(Debug, Deserialize)]
pub struct MembershipResponse {
    pub success: bool,
    pub message: String,
}

/// Request for configuring a failpoint.
#[derive(Debug, Serialize)]
struct ConfigureFailpointRequest {
    name: String,
    action: String,
}

/// Response from failpoint configuration.
#[derive(Debug, Deserialize)]
struct ConfigureFailpointResponse {
    success: bool,
    message: String,
}

/// Configuration for a single node in the cluster.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub node_id: u64,
    pub api_port: u16,
    pub raft_port: u16,
    /// Optional internal API port. When set, enables internal cluster API.
    pub internal_api_port: Option<u16>,
}

/// A single node in the cluster.
pub struct ClusterNode {
    pub config: NodeConfig,
    pub child: Option<Child>,
    pub data_dir: TempDir,
    pub data_path: PathBuf,
    pub metadata_path: PathBuf,
    pub config_path: PathBuf,
    pub stderr_path: PathBuf,
    pub client: Client,
}

impl ClusterNode {
    async fn wait_ready(&self) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/health", self.config.api_port);
        let start = Instant::now();

        loop {
            if start.elapsed() > Duration::from_secs(30) {
                anyhow::bail!(
                    "Node {} failed to start within 30 seconds",
                    self.config.node_id
                );
            }

            match reqwest::get(&url).await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    }

    pub fn pid(&self) -> Option<i32> {
        self.child.as_ref().map(|c| c.id() as i32)
    }
}

/// Multi-node cluster environment for testing Raft consensus.
pub struct ClusterEnv {
    nodes: HashMap<u64, ClusterNode>,
    node_count: usize,
}

impl ClusterEnv {
    /// Creates and starts a new 3-node cluster.
    pub async fn new_3_node() -> Result<Self> {
        Self::new(3).await
    }

    /// Creates and starts a cluster with the specified number of nodes.
    pub async fn new(node_count: usize) -> Result<Self> {
        // Build binary first (without failpoints)
        ensure_binary_built()?;
        Self::new_without_build(node_count).await
    }

    /// Creates and starts a cluster assuming binary is already built.
    /// Use this when you've already built with specific features (e.g., failpoints).
    pub async fn new_without_build(node_count: usize) -> Result<Self> {
        assert!(node_count >= 1, "Must have at least 1 node");

        // Allocate ports for all nodes
        let mut configs = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let node_id = (i + 1) as u64;
            let api_port = find_free_port()?;
            let raft_port = find_free_port()?;
            configs.push(NodeConfig {
                node_id,
                api_port,
                raft_port,
                internal_api_port: None,
            });
        }

        // Generate peer strings for cluster configuration
        let peer_strings: Vec<String> = configs
            .iter()
            .map(|c| format!("{}:127.0.0.1:{}", c.node_id, c.raft_port))
            .collect();

        // Start all nodes
        let mut nodes = HashMap::new();
        for config in &configs {
            let node = start_node(config, &peer_strings).await?;
            nodes.insert(config.node_id, node);
        }

        let cluster = Self { nodes, node_count };

        // Initialize cluster on node 1 (bootstrap)
        cluster.initialize_cluster().await?;

        // Wait for leader election
        cluster.wait_for_leader(Duration::from_secs(30)).await?;

        Ok(cluster)
    }

    /// Initialize the cluster by calling /cluster/initialize on node 1.
    async fn initialize_cluster(&self) -> Result<()> {
        let node = self.nodes.get(&1).context("Node 1 not found")?;

        // Give nodes time to start their Raft servers
        tokio::time::sleep(RAFT_STARTUP_DELAY).await;

        // Check if cluster is already initialized (single-node clusters auto-initialize)
        if let Ok(status) = self.get_node_status(1).await
            && let Some(leader_id) = status.current_leader
        {
            tracing::info!("Cluster already initialized with leader {}", leader_id);
            return Ok(());
        }

        // Build members list for initialization
        // Format: node_id:host:raft_port:http_port
        let members: Vec<String> = self
            .nodes
            .values()
            .map(|n| {
                format!(
                    "{}:127.0.0.1:{}:{}",
                    n.config.node_id, n.config.raft_port, n.config.api_port
                )
            })
            .collect();

        // Call /cluster/initialize on node 1 to bootstrap the cluster
        let url = format!(
            "http://127.0.0.1:{}/cluster/initialize",
            node.config.api_port
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&serde_json::json!({ "members": members }))
            .send()
            .await
            .context("Failed to send initialize request")?;

        // 409 Conflict means already initialized - that's ok
        if response.status() == reqwest::StatusCode::CONFLICT {
            tracing::info!("Cluster already initialized (409 response)");
            return Ok(());
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to initialize cluster: {} - {}", status, body);
        }

        tracing::info!(
            "Cluster initialized with {} nodes, waiting for leader election",
            members.len()
        );

        Ok(())
    }

    /// Wait for a leader to be elected.
    /// Only considers running nodes and ensures the leader is also running.
    pub async fn wait_for_leader(&self, timeout: Duration) -> Result<u64> {
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                // Print stderr logs from all nodes to help debug
                for node in self.nodes.values() {
                    if let Ok(content) = std::fs::read_to_string(&node.stderr_path) {
                        let last_lines: Vec<&str> = content.lines().rev().take(50).collect();
                        tracing::error!(
                            "Node {} stderr (last 50 lines):\n{}",
                            node.config.node_id,
                            last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
                        );
                    }
                }
                anyhow::bail!("Timeout waiting for leader election");
            }

            // Only check running nodes
            for node in self.nodes.values() {
                // Skip nodes that are not running
                if node.child.is_none() {
                    continue;
                }

                if let Ok(status) = self.get_node_status(node.config.node_id).await
                    && let Some(leader_id) = status.current_leader
                    && let Some(leader_node) = self.nodes.get(&leader_id)
                    && leader_node.child.is_some()
                {
                    // Verify the reported leader is actually in Leader state
                    // by checking the leader's own status
                    if let Ok(leader_status) = self.get_node_status(leader_id).await
                        && leader_status.state == "leader"
                    {
                        tracing::info!("Leader elected: node {}", leader_id);
                        return Ok(leader_id);
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Get the status of a specific node.
    pub async fn get_node_status(&self, node_id: u64) -> Result<ClusterStatusResponse> {
        let node = self.nodes.get(&node_id).context("Node not found")?;
        let url = format!("http://127.0.0.1:{}/cluster/status", node.config.api_port);

        let response = reqwest::get(&url)
            .await
            .context("Failed to get cluster status")?;

        if !response.status().is_success() {
            anyhow::bail!("Cluster status request failed: {}", response.status());
        }

        let status: ClusterStatusResponse = response.json().await?;
        Ok(status)
    }

    /// Get the current leader ID, if known.
    pub async fn get_leader(&self) -> Option<u64> {
        for node in self.nodes.values() {
            if let Ok(status) = self.get_node_status(node.config.node_id).await
                && status.current_leader.is_some()
            {
                return status.current_leader;
            }
        }
        None
    }

    /// Wait for a node to become a cluster member (voter or learner).
    /// Polls until the node appears in membership or timeout is reached.
    pub async fn wait_for_membership(&self, node_id: u64, timeout: Duration) -> Result<()> {
        let start = Instant::now();

        loop {
            if start.elapsed() > timeout {
                anyhow::bail!(
                    "Timeout waiting for node {} to join cluster membership",
                    node_id
                );
            }

            // Check any running node for membership info
            for node in self.nodes.values() {
                if node.child.is_none() {
                    continue;
                }

                if let Ok(status) = self.get_node_status(node.config.node_id).await
                    && (status.voters.contains(&node_id) || status.learners.contains(&node_id))
                {
                    tracing::info!(
                        "Node {} is now a cluster member (voters: {:?}, learners: {:?})",
                        node_id,
                        status.voters,
                        status.learners
                    );
                    return Ok(());
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Kill a specific node (simulating crash).
    pub fn kill_node(&mut self, node_id: u64) -> Result<()> {
        let node = self.nodes.get_mut(&node_id).context("Node not found")?;

        if let Some(ref mut child) = node.child {
            let pid = Pid::from_raw(child.id() as i32);
            kill(pid, Signal::SIGKILL).context("Failed to send SIGKILL")?;
            child.wait().context("Failed to wait for child process")?;
        }
        node.child = None;

        tracing::info!("Killed node {}", node_id);
        Ok(())
    }

    /// Restart a previously killed node.
    pub async fn restart_node(&mut self, node_id: u64) -> Result<()> {
        let node = self.nodes.get_mut(&node_id).context("Node not found")?;

        if node.child.is_some() {
            anyhow::bail!("Node {} is already running", node_id);
        }

        // Capture stderr to a file for debugging restart failures
        let stderr_path = node.data_dir.path().join("restart_stderr.log");
        let stderr_file =
            std::fs::File::create(&stderr_path).context("Failed to create stderr log file")?;

        let child = Command::new(get_binary_path())
            .env("SAVE_CONFIG", &node.config_path)
            .env("RUST_LOG", "info,save_metadata::raft=debug")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::from(stderr_file))
            .spawn()
            .context("Failed to restart node")?;

        node.child = Some(child);

        // Try to wait for ready, but if it fails, show the stderr log
        match node.wait_ready().await {
            Ok(()) => {
                tracing::info!("Restarted node {}", node_id);
                Ok(())
            }
            Err(e) => {
                // Read and log stderr for debugging
                if let Ok(stderr_content) = std::fs::read_to_string(&stderr_path) {
                    tracing::error!("Server stderr on restart failure:\n{}", stderr_content);
                }
                Err(e)
            }
        }
    }

    /// Get a client for a specific node by ID.
    pub fn node_client(&self, node_id: u64) -> Option<&Client> {
        self.nodes.get(&node_id).map(|n| &n.client)
    }

    /// Get a client for the current leader.
    pub async fn leader_client(&self) -> Result<&Client> {
        let leader_id = self.get_leader().await.context("No leader available")?;
        self.nodes
            .get(&leader_id)
            .map(|n| &n.client)
            .context("Leader node not found")
    }

    /// Get all node IDs in the cluster.
    pub fn node_ids(&self) -> Vec<u64> {
        self.nodes.keys().copied().collect()
    }

    /// Get the number of nodes in the cluster.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Check if a node is running.
    pub fn is_node_running(&self, node_id: u64) -> bool {
        self.nodes
            .get(&node_id)
            .map(|n| n.child.is_some())
            .unwrap_or(false)
    }

    /// Get data directory path for a node.
    pub fn data_dir(&self, node_id: u64) -> Option<&Path> {
        self.nodes.get(&node_id).map(|n| n.data_dir.path())
    }

    /// Get the API port for a node.
    pub fn api_port(&self, node_id: u64) -> Option<u16> {
        self.nodes.get(&node_id).map(|n| n.config.api_port)
    }

    /// Add a learner node to the cluster via the leader.
    pub async fn add_learner(&self, node_id: u64, raft_addr: &str) -> Result<MembershipResponse> {
        let leader_id = self.get_leader().await.context("No leader available")?;
        let node = self
            .nodes
            .get(&leader_id)
            .context("Leader node not found")?;

        let url = format!("http://127.0.0.1:{}/cluster/members", node.config.api_port);
        let node_spec = format!("{}:{}", node_id, raft_addr);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&serde_json::json!({ "node": node_spec }))
            .send()
            .await
            .context("Failed to send add_learner request")?;

        let resp: MembershipResponse = response.json().await?;
        Ok(resp)
    }

    /// Promote learner nodes to voters via the leader.
    pub async fn promote_voters(&self, node_ids: Vec<u64>) -> Result<MembershipResponse> {
        let leader_id = self.get_leader().await.context("No leader available")?;
        let node = self
            .nodes
            .get(&leader_id)
            .context("Leader node not found")?;

        let url = format!(
            "http://127.0.0.1:{}/cluster/members/promote",
            node.config.api_port
        );

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&serde_json::json!({ "node_ids": node_ids }))
            .send()
            .await
            .context("Failed to send promote_voters request")?;

        let resp: MembershipResponse = response.json().await?;
        Ok(resp)
    }

    /// Remove a node from the cluster via the leader.
    pub async fn remove_node(&self, node_id: u64) -> Result<MembershipResponse> {
        let leader_id = self.get_leader().await.context("No leader available")?;
        let node = self
            .nodes
            .get(&leader_id)
            .context("Leader node not found")?;

        let url = format!(
            "http://127.0.0.1:{}/cluster/members/{}",
            node.config.api_port, node_id
        );

        let client = reqwest::Client::new();
        let response = client
            .delete(&url)
            .send()
            .await
            .context("Failed to send remove_node request")?;

        let resp: MembershipResponse = response.json().await?;
        Ok(resp)
    }

    /// Start and add a new node to the cluster manually (as learner).
    /// Use `add_auto_joining_node` for automatic cluster join.
    pub async fn add_new_node(&mut self) -> Result<u64> {
        // Find max existing node_id and add 1
        let new_node_id = self.nodes.keys().max().unwrap_or(&0) + 1;

        // Allocate ports
        let api_port = find_free_port()?;
        let raft_port = find_free_port()?;

        let config = NodeConfig {
            node_id: new_node_id,
            api_port,
            raft_port,
            internal_api_port: None,
        };

        // Build peer list from existing nodes
        let peers: Vec<String> = self
            .nodes
            .values()
            .map(|n| format!("{}:127.0.0.1:{}", n.config.node_id, n.config.raft_port))
            .collect();

        // Start the new node
        let node = start_node(&config, &peers).await?;
        let raft_addr = format!("127.0.0.1:{}", raft_port);

        self.nodes.insert(new_node_id, node);

        // Add it as a learner via the leader
        let response = self.add_learner(new_node_id, &raft_addr).await?;
        if !response.success {
            anyhow::bail!("Failed to add learner: {}", response.message);
        }

        tracing::info!("Added new node {} as learner", new_node_id);
        Ok(new_node_id)
    }

    /// Get the Raft port for a node.
    pub fn raft_port(&self, node_id: u64) -> Option<u16> {
        self.nodes.get(&node_id).map(|n| n.config.raft_port)
    }

    /// Print server stderr logs for a specific node (for debugging).
    pub fn print_node_stderr(&self, node_id: u64) {
        if let Some(node) = self.nodes.get(&node_id) {
            if let Ok(content) = std::fs::read_to_string(&node.stderr_path) {
                tracing::info!("=== Server stderr for node {} ===\n{}", node_id, content);
            } else {
                tracing::warn!("Could not read stderr for node {}", node_id);
            }
        }
    }

    /// Creates and starts a 3-node cluster with internal API enabled.
    /// Use this for tests that need graceful scaling (auto-join/graceful-leave).
    pub async fn new_3_node_with_internal_api() -> Result<Self> {
        ensure_binary_built()?;
        Self::new_with_internal_api(3).await
    }

    /// Creates a cluster with internal API enabled on all nodes.
    async fn new_with_internal_api(node_count: usize) -> Result<Self> {
        assert!(node_count >= 1, "Must have at least 1 node");

        // Allocate ports for all nodes
        let mut configs = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let node_id = (i + 1) as u64;
            let api_port = find_free_port()?;
            let raft_port = find_free_port()?;
            let internal_api_port = find_free_port()?;
            configs.push(NodeConfig {
                node_id,
                api_port,
                raft_port,
                internal_api_port: Some(internal_api_port),
            });
        }

        // Generate peer strings for cluster configuration
        let peer_strings: Vec<String> = configs
            .iter()
            .map(|c| format!("{}:127.0.0.1:{}:{}", c.node_id, c.raft_port, c.api_port))
            .collect();

        // Start all nodes
        let mut nodes = HashMap::new();
        for config in &configs {
            let node = start_node(config, &peer_strings).await?;
            nodes.insert(config.node_id, node);
        }

        let cluster = Self { nodes, node_count };

        // Initialize cluster on node 1 (bootstrap)
        cluster.initialize_cluster().await?;

        // Wait for leader election
        cluster.wait_for_leader(Duration::from_secs(30)).await?;

        Ok(cluster)
    }

    /// Start and add a new node that will auto-join the cluster.
    /// The node joins automatically during startup when seed_nodes is configured.
    pub async fn add_auto_joining_node(&mut self) -> Result<u64> {
        // Find max existing node_id and add 1
        let new_node_id = self.nodes.keys().max().unwrap_or(&0) + 1;

        // Allocate ports
        let api_port = find_free_port()?;
        let raft_port = find_free_port()?;
        let internal_api_port = find_free_port()?;

        let config = NodeConfig {
            node_id: new_node_id,
            api_port,
            raft_port,
            internal_api_port: Some(internal_api_port),
        };

        // Build peer list from existing nodes (for discovery)
        let peers: Vec<String> = self
            .nodes
            .values()
            .map(|n| {
                format!(
                    "{}:127.0.0.1:{}:{}",
                    n.config.node_id, n.config.raft_port, n.config.api_port
                )
            })
            .collect();

        // Start the new node - it will auto-join during startup
        let node = start_node(&config, &peers).await?;
        self.nodes.insert(new_node_id, node);

        tracing::info!(
            "Started new node {} (auto-joins via seed_nodes)",
            new_node_id
        );
        Ok(new_node_id)
    }

    /// Gracefully stop a node (SIGTERM). Node will leave cluster before shutdown.
    pub fn graceful_stop_node(&mut self, node_id: u64) -> Result<()> {
        let node = self.nodes.get_mut(&node_id).context("Node not found")?;

        if let Some(ref mut child) = node.child {
            let pid = Pid::from_raw(child.id() as i32);
            // Use SIGTERM for graceful shutdown (not SIGKILL)
            kill(pid, Signal::SIGTERM).context("Failed to send SIGTERM")?;
            // Wait for the process to exit
            child.wait().context("Failed to wait for child process")?;
        }
        node.child = None;

        tracing::info!("Gracefully stopped node {}", node_id);
        Ok(())
    }
}

/// TestEnvironment implementation for single-node crash tests.
///
/// Uses `ClusterEnv::new(1)` to run crash tests with Raft enabled.
/// This ensures crash tests verify the actual production code path.
#[async_trait::async_trait]
impl TestEnvironment for ClusterEnv {
    async fn setup() -> Result<Self> {
        // Build binary with failpoints enabled for crash testing
        ensure_binary_built_with_failpoints()?;

        // Use a single-node cluster for crash tests (skip rebuild to keep failpoints)
        Self::new_without_build(1).await
    }

    async fn configure_failpoint(&self, name: &str, action: &str) -> Result<()> {
        // For single-node crash tests, always use node 1
        let node = self.nodes.get(&1).context("Node 1 not found")?;
        let url = format!(
            "http://127.0.0.1:{}/_failpoint/configure",
            node.config.api_port
        );

        let req = ConfigureFailpointRequest {
            name: name.to_string(),
            action: action.to_string(),
        };

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .json(&req)
            .send()
            .await
            .context("Failed to send failpoint configuration")?;

        let status = response.status();

        // Check for 404 - means binary wasn't built with failpoints feature
        if status == reqwest::StatusCode::NOT_FOUND {
            anyhow::bail!(
                "Failpoint endpoint not found. Make sure save-api is built with --features failpoints"
            );
        }

        let body_text = response
            .text()
            .await
            .context("Failed to read response body")?;

        let body: ConfigureFailpointResponse = serde_json::from_str(&body_text)
            .with_context(|| format!("Failed to parse failpoint response: '{}'", body_text))?;

        if !status.is_success() || !body.success {
            anyhow::bail!("Failed to configure failpoint: {}", body.message);
        }

        Ok(())
    }

    async fn wait_for_failpoint(&self, _name: &str) -> Result<()> {
        // When using "pause" action, the failpoint blocks the request thread.
        // Wait for the request to reach the failpoint and block.
        tokio::time::sleep(Duration::from_secs(2)).await;
        Ok(())
    }

    async fn crash_at_failpoint(&mut self, name: &str) -> Result<()> {
        // Kill node 1 with SIGKILL to simulate crash
        self.kill_node(1)?;

        // Remove the failpoint
        fail::remove(name);

        Ok(())
    }

    async fn restart(&mut self) -> Result<()> {
        self.restart_node(1).await
    }

    fn client(&self) -> &Client {
        // For single-node crash tests, always use node 1's client
        self.nodes
            .get(&1)
            .map(|n| &n.client)
            .expect("Node 1 not found")
    }

    async fn verify_consistency(&self) -> Result<()> {
        let node = self.nodes.get(&1).context("Node 1 not found")?;

        // Stop the node to avoid RocksDB lock conflicts during verification
        if let Some(ref child) = node.child {
            let pid = Pid::from_raw(child.id() as i32);
            kill(pid, Signal::SIGTERM).ok();
            // Wait for graceful shutdown
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        verify_no_phantom_objects(&node.data_path, &node.metadata_path).await?;
        verify_no_orphans_or_gc_pending(&node.data_path, &node.metadata_path).await?;
        Ok(())
    }

    fn data_dir(&self) -> &Path {
        &self.nodes.get(&1).expect("Node 1 not found").data_path
    }

    fn metadata_dir(&self) -> &Path {
        &self.nodes.get(&1).expect("Node 1 not found").metadata_path
    }
}

impl Drop for ClusterEnv {
    fn drop(&mut self) {
        for (node_id, node) in self.nodes.iter_mut() {
            if let Some(ref mut child) = node.child {
                tracing::debug!("Cleaning up node {}", node_id);
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

async fn start_node(config: &NodeConfig, peers: &[String]) -> Result<ClusterNode> {
    let data_dir = tempfile::tempdir()?;

    // Filter out this node from peers list
    let other_peers: Vec<String> = peers
        .iter()
        .filter(|p| !p.starts_with(&format!("{}:", config.node_id)))
        .cloned()
        .collect();

    let peers_toml = if other_peers.is_empty() {
        "[]".to_string()
    } else {
        format!("[\"{}\"]", other_peers.join("\", \""))
    };

    // Optional internal API config section
    let internal_api_section = config
        .internal_api_port
        .map(|port| {
            format!(
                r#"
[cluster.internal_api]
bind_addr = "127.0.0.1:{}"
require_auth = false
"#,
                port
            )
        })
        .unwrap_or_default();

    // Create config file
    let config_content = format!(
        r#"
[server]
bind_address = "127.0.0.1:{api_port}"

[storage]
data_path = "{data_path}/data"
metadata_path = "{data_path}/metadata"
gc_interval_secs = 10
gc_temp_file_max_age_secs = 60

[credentials]
access_key = "test-access-key"
secret_key = "test-secret-key"

[cluster]
node_id = {node_id}
raft_bind_addr = "127.0.0.1:{raft_port}"
seed_nodes = {seed_nodes}
consistency_mode = "eventual"
{internal_api}
"#,
        api_port = config.api_port,
        data_path = data_dir.path().display(),
        node_id = config.node_id,
        raft_port = config.raft_port,
        seed_nodes = peers_toml,
        internal_api = internal_api_section,
    );

    let config_path = data_dir.path().join("config.toml");
    std::fs::write(&config_path, config_content)?;

    // Capture stderr to a log file for debugging
    let stderr_path = data_dir.path().join("stderr.log");
    let stderr_file =
        std::fs::File::create(&stderr_path).context("Failed to create stderr log file")?;

    // Spawn server with appropriate log level
    let log_level = if config.internal_api_port.is_some() {
        "info,save_metadata::raft=debug,save_api::scaling=debug"
    } else {
        "info,save_metadata::raft=debug"
    };

    let child = Command::new(get_binary_path())
        .env("SAVE_CONFIG", &config_path)
        .env("RUST_LOG", log_level)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_file))
        .spawn()
        .context("Failed to spawn save-api")?;

    let client = create_client(config.api_port).await;

    let data_path = data_dir.path().join("data");
    let metadata_path = data_dir.path().join("metadata");

    let node = ClusterNode {
        config: config.clone(),
        child: Some(child),
        data_dir,
        data_path,
        metadata_path,
        config_path,
        stderr_path,
        client,
    };

    node.wait_ready().await?;

    tracing::info!(
        "Started node {} on api_port={}, raft_port={}{}",
        config.node_id,
        config.api_port,
        config.raft_port,
        config
            .internal_api_port
            .map(|p| format!(", internal_api_port={}", p))
            .unwrap_or_default()
    );

    Ok(node)
}

fn find_free_port() -> Result<u16> {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn get_binary_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target/debug/save-api")
}

fn ensure_binary_built() -> Result<()> {
    let status = Command::new("cargo")
        .args(["build", "-p", "save-api"])
        .stdout(std::process::Stdio::null())
        .status()
        .context("Failed to run cargo build")?;

    if !status.success() {
        anyhow::bail!("Failed to build save-api");
    }

    Ok(())
}

/// Build binary with failpoints enabled (used by crash tests).
fn ensure_binary_built_with_failpoints() -> Result<()> {
    let status = Command::new("cargo")
        .args(["build", "-p", "save-api", "--features", "failpoints"])
        .stdout(std::process::Stdio::null())
        .status()
        .context("Failed to run cargo build with failpoints")?;

    if !status.success() {
        anyhow::bail!("Failed to build save-api with failpoints");
    }

    Ok(())
}

async fn create_client(port: u16) -> Client {
    let credentials = Credentials::new("test-access-key", "test-secret-key", None, None, "static");

    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .force_path_style(true)
        .build();

    Client::from_conf(s3_config)
}

#[cfg(all(test, feature = "cluster_tests"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cluster_env_setup() {
        let cluster = ClusterEnv::new_3_node().await.unwrap();
        assert_eq!(cluster.node_count(), 3);

        // Check all nodes are running
        for node_id in cluster.node_ids() {
            assert!(cluster.is_node_running(node_id));
        }

        // Verify leader was elected
        let leader = cluster.get_leader().await;
        assert!(leader.is_some());
    }
}
