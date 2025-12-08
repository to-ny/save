//! Replication coordinator for quorum writes across nodes.

use super::client::ReplicationClient;
use super::health::HealthChecker;
use crate::StorageError;
use save_common::{RetryConfig, TlsConfig};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::AsyncRead;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Quorum configuration.
#[derive(Debug, Clone)]
pub struct QuorumConfig {
    /// Number of replicas to write (replication factor).
    pub replication_factor: usize,
    /// Minimum successful writes for quorum.
    pub write_quorum: usize,
    /// Minimum successful reads for quorum (unused in Phase 2).
    pub read_quorum: usize,
}

impl QuorumConfig {
    /// Create config with N replicas, requiring majority for quorum.
    pub fn with_replication_factor(n: usize) -> Self {
        let quorum = (n / 2) + 1;
        Self {
            replication_factor: n,
            write_quorum: quorum,
            read_quorum: 1, // Eventual consistency by default
        }
    }
}

impl Default for QuorumConfig {
    fn default() -> Self {
        Self::with_replication_factor(3)
    }
}

/// Result of a replicated operation.
#[derive(Debug)]
pub struct ReplicationResult {
    /// Number of nodes that succeeded.
    pub success_count: usize,
    /// Number of nodes that failed.
    pub failure_count: usize,
    /// Whether quorum was achieved.
    pub quorum_achieved: bool,
    /// Node IDs that succeeded.
    pub successful_nodes: Vec<u64>,
    /// Node IDs that failed with error messages.
    pub failed_nodes: Vec<(u64, String)>,
}

/// Prepared write state for 2PC.
struct PreparedWrite {
    key: String,
    #[allow(dead_code)]
    checksum: String,
    request_id: u64,
    temp_ids: HashMap<u64, String>, // node_id -> temp_id
}

/// Result of the prepare phase in 2PC.
struct PreparePhaseResult {
    prepared: PreparedWrite,
    success_count: usize,
    successful_nodes: Vec<u64>,
    failed_nodes: Vec<(u64, String)>,
}

/// Coordinates replicated writes across cluster nodes.
pub struct ReplicationCoordinator {
    local_node_id: u64,
    config: QuorumConfig,
    connect_timeout: Duration,
    rpc_timeout: Duration,
    retry_config: RetryConfig,
    tls_config: Option<TlsConfig>,
    health_checker: HealthChecker,
    clients: Arc<RwLock<HashMap<u64, ReplicationClient>>>,
    request_counter: AtomicU64,
}

impl ReplicationCoordinator {
    /// Create a new coordinator with default timeouts.
    pub fn new(local_node_id: u64, config: QuorumConfig) -> Self {
        Self::with_timeouts(
            local_node_id,
            config,
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
    }

    /// Create a new coordinator with custom timeouts.
    pub fn with_timeouts(
        local_node_id: u64,
        config: QuorumConfig,
        connect_timeout: Duration,
        rpc_timeout: Duration,
    ) -> Self {
        Self {
            local_node_id,
            config,
            connect_timeout,
            rpc_timeout,
            retry_config: RetryConfig::default(),
            tls_config: None,
            health_checker: HealthChecker::default(),
            clients: Arc::new(RwLock::new(HashMap::new())),
            request_counter: AtomicU64::new(1),
        }
    }

    /// Set health check timeout for node selection.
    pub fn with_health_check_timeout(mut self, timeout: Duration) -> Self {
        self.health_checker = HealthChecker::with_timeout(timeout);
        self
    }

    /// Set retry configuration for client operations.
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Enable mTLS for node-to-node communication.
    pub fn with_tls(mut self, config: TlsConfig) -> Self {
        self.tls_config = Some(config);
        self
    }

    /// Add a replica node client.
    pub async fn add_node(&self, node_id: u64, addr: String) -> Result<(), StorageError> {
        if node_id == self.local_node_id {
            return Ok(()); // Skip self
        }

        let mut client =
            ReplicationClient::with_timeouts(node_id, addr, self.connect_timeout, self.rpc_timeout)
                .with_retry_config(self.retry_config.clone());

        if let Some(ref tls) = self.tls_config {
            client = client.with_tls(tls)?;
        }

        self.clients.write().await.insert(node_id, client);
        info!(node_id = %node_id, "Added replication client");
        Ok(())
    }

    /// Remove a replica node client.
    pub async fn remove_node(&self, node_id: u64) {
        self.clients.write().await.remove(&node_id);
        info!(node_id = %node_id, "Removed replication client");
    }

    /// Get connected node IDs (excluding self).
    pub async fn connected_nodes(&self) -> Vec<u64> {
        self.clients.read().await.keys().copied().collect()
    }

    /// Select nodes for placement from healthy nodes only.
    /// Returns up to `count - 1` nodes (local counts as one).
    pub async fn select_replica_nodes(&self, count: usize) -> Vec<u64> {
        let needed = count.saturating_sub(1); // -1 for local node
        if needed == 0 {
            return vec![];
        }

        let clients = self.clients.read().await;
        let mut healthy_nodes = Vec::with_capacity(needed);

        for (node_id, client) in clients.iter() {
            if healthy_nodes.len() >= needed {
                break;
            }
            if self.health_checker.is_healthy(client).await {
                healthy_nodes.push(*node_id);
            } else {
                debug!(node_id = %node_id, "Skipping unhealthy node for replica selection");
            }
        }

        healthy_nodes
    }

    /// Check health of a specific node by ID.
    pub async fn health_check(&self, node_id: u64) -> Result<bool, StorageError> {
        let clients = self.clients.read().await;
        if let Some(client) = clients.get(&node_id) {
            Ok(self.health_checker.is_healthy(client).await)
        } else {
            Err(StorageError::Io(std::io::Error::other(format!(
                "Node {} not found",
                node_id
            ))))
        }
    }

    /// Read object from remote replicas with streaming.
    /// Tries healthy nodes until read_quorum successful reads are found.
    /// Returns a streaming reader from the first successful node.
    pub async fn read_from_replica(&self, key: &str) -> Option<Box<dyn AsyncRead + Send + Unpin>> {
        let clients = self.clients.read().await;
        let mut attempts = 0;
        let required = self.config.read_quorum;

        for (node_id, client) in clients.iter() {
            if !self.health_checker.is_healthy(client).await {
                debug!(node_id = %node_id, "Skipping unhealthy node for read");
                continue;
            }

            match client.read_object_stream(key).await {
                Ok(reader) => {
                    attempts += 1;
                    debug!(
                        node_id = %node_id,
                        key = %key,
                        attempts = attempts,
                        required = required,
                        "Read stream opened from replica"
                    );
                    // For read_quorum=1, return immediately
                    // For read_quorum>1, we'd need to verify consistency (Phase 3)
                    if attempts >= required {
                        return Some(Box::new(reader) as Box<dyn AsyncRead + Send + Unpin>);
                    }
                }
                Err(e) => {
                    debug!(node_id = %node_id, key = %key, error = %e, "Failed to read from replica");
                }
            }
        }

        None
    }

    /// Replicate object write to quorum of nodes using 2PC.
    ///
    /// Returns Ok if quorum achieved, Err otherwise.
    pub async fn replicate_write(
        &self,
        key: &str,
        data: Vec<u8>,
        local_write: impl std::future::Future<Output = Result<(), StorageError>>,
    ) -> Result<ReplicationResult, StorageError> {
        let request_id = self.request_counter.fetch_add(1, Ordering::SeqCst);
        let checksum = compute_sha256(&data);

        let replica_nodes = self
            .select_replica_nodes(self.config.replication_factor)
            .await;

        debug!(
            key = %key,
            request_id = %request_id,
            replica_count = replica_nodes.len(),
            "Starting replicated write"
        );

        // Phase 1: Prepare on all replicas
        let prepare_results = self
            .prepare_all(&replica_nodes, key, data, &checksum, request_id)
            .await;

        let prepare_result =
            self.process_prepare_results(key, &checksum, request_id, prepare_results);

        debug!(
            key = %key,
            request_id = %request_id,
            success_count = prepare_result.success_count,
            "Replicated write completed"
        );

        self.complete_2pc_write(prepare_result, local_write).await
    }

    /// Replicate object write using streaming for large objects.
    /// Avoids buffering entire object in memory.
    pub async fn replicate_write_streaming(
        &self,
        key: &str,
        file_path: &std::path::Path,
        checksum: &str,
        local_write: impl std::future::Future<Output = Result<(), StorageError>>,
    ) -> Result<ReplicationResult, StorageError> {
        let request_id = self.request_counter.fetch_add(1, Ordering::SeqCst);

        let replica_nodes = self
            .select_replica_nodes(self.config.replication_factor)
            .await;

        debug!(
            key = %key,
            request_id = %request_id,
            replica_count = replica_nodes.len(),
            "Starting streaming replicated write"
        );

        // Phase 1: Stream prepare on all replicas
        let prepare_results = self
            .stream_prepare_all(&replica_nodes, key, file_path, checksum, request_id)
            .await;

        let prepare_result =
            self.process_prepare_results(key, checksum, request_id, prepare_results);

        debug!(
            key = %key,
            request_id = %request_id,
            success_count = prepare_result.success_count,
            "Streaming replicated write completed"
        );

        self.complete_2pc_write(prepare_result, local_write).await
    }

    /// Replicate object deletion to all nodes.
    pub async fn replicate_delete(
        &self,
        key: &str,
        local_delete: impl std::future::Future<Output = Result<(), StorageError>>,
    ) -> Result<ReplicationResult, StorageError> {
        let request_id = self.request_counter.fetch_add(1, Ordering::SeqCst);

        // Delete locally first
        local_delete.await?;

        // Delete on all replicas (best effort)
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        debug!(key = %key, client_count = clients.len(), "Replicating delete to nodes");

        for (node_id, client) in clients.iter() {
            let node_id = *node_id;
            let client = client.clone();
            let key = key.to_string();

            debug!(node_id = %node_id, key = %key, "Spawning delete task for node");
            handles.push(tokio::spawn(async move {
                let result = client.delete_object(&key, request_id).await;
                (node_id, result)
            }));
        }

        drop(clients);

        let mut success_count = 1; // Local
        let mut successful_nodes = vec![self.local_node_id];
        let mut failed_nodes = Vec::new();

        for handle in handles {
            if let Ok((node_id, result)) = handle.await {
                match result {
                    Ok(_) => {
                        debug!(node_id = %node_id, "Delete succeeded on replica");
                        success_count += 1;
                        successful_nodes.push(node_id);
                    }
                    Err(e) => {
                        warn!(node_id = %node_id, error = %e, "Delete failed on replica");
                        failed_nodes.push((node_id, e.to_string()));
                    }
                }
            }
        }

        Ok(ReplicationResult {
            success_count,
            failure_count: failed_nodes.len(),
            quorum_achieved: success_count >= self.config.write_quorum,
            successful_nodes,
            failed_nodes,
        })
    }

    async fn prepare_all(
        &self,
        nodes: &[u64],
        key: &str,
        data: Vec<u8>,
        checksum: &str,
        request_id: u64,
    ) -> Vec<(u64, Result<String, StorageError>)> {
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        for node_id in nodes {
            if let Some(client) = clients.get(node_id) {
                let node_id = *node_id;
                let client = client.clone();
                let key = key.to_string();
                let data = data.clone();
                let checksum = checksum.to_string();

                handles.push(tokio::spawn(async move {
                    let result = client
                        .prepare_object(&key, data, &checksum, request_id)
                        .await;
                    (node_id, result)
                }));
            }
        }

        drop(clients);

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    async fn stream_prepare_all(
        &self,
        nodes: &[u64],
        key: &str,
        file_path: &std::path::Path,
        checksum: &str,
        request_id: u64,
    ) -> Vec<(u64, Result<String, StorageError>)> {
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        for node_id in nodes {
            if let Some(client) = clients.get(node_id) {
                let node_id = *node_id;
                let client = client.clone();
                let key = key.to_string();
                let checksum = checksum.to_string();
                let path = file_path.to_path_buf();

                handles.push(tokio::spawn(async move {
                    let result = client
                        .stream_prepare_object(&key, request_id, &path, &checksum)
                        .await;
                    (node_id, result)
                }));
            }
        }

        drop(clients);

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    async fn commit_all(&self, prepared: &PreparedWrite) -> Vec<(u64, Result<(), StorageError>)> {
        let clients = self.clients.read().await;
        let mut handles = Vec::new();

        for (node_id, temp_id) in &prepared.temp_ids {
            if let Some(client) = clients.get(node_id) {
                let node_id = *node_id;
                let client = client.clone();
                let key = prepared.key.clone();
                let temp_id = temp_id.clone();
                let request_id = prepared.request_id;

                handles.push(tokio::spawn(async move {
                    let result = client.commit_object(&key, &temp_id, request_id).await;
                    (node_id, result)
                }));
            }
        }

        drop(clients);

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    async fn abort_all(&self, prepared: &PreparedWrite) {
        let clients = self.clients.read().await;

        // Collect all abort tasks upfront to avoid borrowing issues
        let abort_tasks: Vec<_> = prepared
            .temp_ids
            .iter()
            .filter_map(|(node_id, temp_id)| {
                clients.get(node_id).map(|client| {
                    let node_id = *node_id;
                    let client = client.clone();
                    let key = prepared.key.clone();
                    let temp_id = temp_id.clone();
                    let request_id = prepared.request_id;
                    (node_id, client, key, temp_id, request_id)
                })
            })
            .collect();

        drop(clients);

        // Fire and forget aborts
        for (node_id, client, key, temp_id, request_id) in abort_tasks {
            tokio::spawn(async move {
                if let Err(e) = client.abort_object(&key, &temp_id, request_id).await {
                    warn!(node_id = %node_id, error = %e, "Abort failed");
                }
            });
        }
    }

    /// Process prepare results into a PreparePhaseResult.
    fn process_prepare_results(
        &self,
        key: &str,
        checksum: &str,
        request_id: u64,
        prepare_results: Vec<(u64, Result<String, StorageError>)>,
    ) -> PreparePhaseResult {
        let mut prepared = PreparedWrite {
            key: key.to_string(),
            checksum: checksum.to_string(),
            request_id,
            temp_ids: HashMap::new(),
        };

        let mut success_count = 1; // Local always participates
        let mut successful_nodes = vec![self.local_node_id];
        let mut failed_nodes = Vec::new();

        for (node_id, result) in prepare_results {
            match result {
                Ok(temp_id) => {
                    prepared.temp_ids.insert(node_id, temp_id);
                    successful_nodes.push(node_id);
                    success_count += 1;
                }
                Err(e) => {
                    failed_nodes.push((node_id, e.to_string()));
                }
            }
        }

        PreparePhaseResult {
            prepared,
            success_count,
            successful_nodes,
            failed_nodes,
        }
    }

    /// Complete 2PC write after prepare phase.
    /// Handles quorum check, local write, commit phase, and result construction.
    async fn complete_2pc_write(
        &self,
        prepare_result: PreparePhaseResult,
        local_write: impl std::future::Future<Output = Result<(), StorageError>>,
    ) -> Result<ReplicationResult, StorageError> {
        let PreparePhaseResult {
            prepared,
            success_count,
            successful_nodes,
            failed_nodes,
        } = prepare_result;

        // Check quorum before proceeding
        if success_count < self.config.write_quorum {
            self.abort_all(&prepared).await;
            return Ok(ReplicationResult {
                success_count,
                failure_count: failed_nodes.len(),
                quorum_achieved: false,
                successful_nodes,
                failed_nodes,
            });
        }

        // Write locally
        if let Err(e) = local_write.await {
            self.abort_all(&prepared).await;
            return Err(e);
        }

        // Phase 2: Commit on all prepared replicas
        let commit_results = self.commit_all(&prepared).await;
        for (node_id, result) in commit_results {
            if let Err(e) = result {
                warn!(node_id = %node_id, error = %e, "Commit failed after prepare");
            }
        }

        Ok(ReplicationResult {
            success_count,
            failure_count: failed_nodes.len(),
            quorum_achieved: true,
            successful_nodes,
            failed_nodes,
        })
    }
}

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

impl std::fmt::Debug for ReplicationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicationCoordinator")
            .field("local_node_id", &self.local_node_id)
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quorum_config_defaults() {
        let config = QuorumConfig::default();
        assert_eq!(config.replication_factor, 3);
        assert_eq!(config.write_quorum, 2);
        assert_eq!(config.read_quorum, 1);
    }

    #[test]
    fn test_quorum_config_factor_5() {
        let config = QuorumConfig::with_replication_factor(5);
        assert_eq!(config.replication_factor, 5);
        assert_eq!(config.write_quorum, 3);
    }

    #[test]
    fn test_quorum_config_factor_1() {
        let config = QuorumConfig::with_replication_factor(1);
        assert_eq!(config.replication_factor, 1);
        assert_eq!(config.write_quorum, 1);
    }

    #[tokio::test]
    async fn test_coordinator_creation() {
        let coord = ReplicationCoordinator::new(1, QuorumConfig::default());
        assert!(coord.connected_nodes().await.is_empty());
    }

    #[tokio::test]
    async fn test_select_replica_nodes_empty() {
        let coord = ReplicationCoordinator::new(1, QuorumConfig::default());
        let nodes = coord.select_replica_nodes(3).await;
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn test_replication_result_quorum_achieved() {
        let result = ReplicationResult {
            success_count: 2,
            failure_count: 1,
            quorum_achieved: true,
            successful_nodes: vec![1, 2],
            failed_nodes: vec![(3, "timeout".to_string())],
        };

        assert!(result.quorum_achieved);
        assert_eq!(result.success_count, 2);
        assert_eq!(result.failure_count, 1);
    }

    #[tokio::test]
    async fn test_local_only_write_achieves_quorum_for_factor_1() {
        let config = QuorumConfig::with_replication_factor(1);
        let coord = ReplicationCoordinator::new(1, config);

        let result = coord
            .replicate_write("test/key", b"data".to_vec(), async { Ok(()) })
            .await
            .unwrap();

        assert!(result.quorum_achieved);
        assert_eq!(result.success_count, 1);
        assert!(result.successful_nodes.contains(&1));
    }

    #[tokio::test]
    async fn test_local_write_failure_aborts() {
        let config = QuorumConfig::with_replication_factor(1);
        let coord = ReplicationCoordinator::new(1, config);

        let result = coord
            .replicate_write("test/key", b"data".to_vec(), async {
                Err(StorageError::Io(std::io::Error::other("disk full")))
            })
            .await;

        assert!(result.is_err());
    }
}
