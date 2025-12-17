//! Automatic cluster scaling: auto-join on startup and graceful leave on shutdown.

use save_common::cluster::parse_peer;
use save_common::config::ClusterConfig;
use save_metadata::raft::RaftNode;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Errors that can occur during scaling operations.
#[derive(Debug, thiserror::Error)]
pub enum ScalingError {
    #[error("No peers configured")]
    NoPeers,
    #[error("Failed to connect to any peer: {0}")]
    ConnectionFailed(String),
    #[error("No leader available in cluster")]
    NoLeader,
    #[error("Join operation failed: {0}")]
    JoinFailed(String),
    #[error("Leave operation failed: {0}")]
    LeaveFailed(String),
    #[error("Operation timed out")]
    Timeout,
    #[error("Leadership transfer timed out after {0} seconds")]
    LeadershipTransferTimeout(u64),
    #[error("Failed to remove self from voter set: {0}")]
    VoterRemovalFailed(String),
    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),
}

/// Result type for scaling operations.
pub type Result<T> = std::result::Result<T, ScalingError>;

/// Response from /cluster/status endpoint.
#[derive(Debug, Deserialize)]
struct ClusterStatusResponse {
    #[allow(dead_code)]
    node_id: u64,
    current_leader: Option<u64>,
    #[allow(dead_code)]
    voters: Vec<u64>,
    #[allow(dead_code)]
    learners: Vec<u64>,
}

/// Response from membership operations.
#[derive(Debug, Deserialize)]
struct MembershipResponse {
    success: bool,
    message: String,
}

/// Request body for adding a learner.
#[derive(Debug, Serialize)]
struct AddLearnerRequest {
    node: String,
}

/// Request body for promoting voters.
#[derive(Debug, Serialize)]
struct PromoteVotersRequest {
    node_ids: Vec<u64>,
}

/// Auto-join cluster on startup. Returns Ok(true) if joined, Ok(false) if already a member.
pub async fn auto_join(
    raft_node: &RaftNode,
    config: &ClusterConfig,
    http_addr: &str,
) -> Result<bool> {
    if config.seed_nodes.is_empty() {
        return Err(ScalingError::NoPeers);
    }

    let timeout = Duration::from_secs(config.join_timeout_secs);
    let node_id = config.node_id;
    let raft_addr = format!("http://{}", config.raft_bind_addr);
    let max_retries = config.join_max_retries;
    let learner_catchup_delay = Duration::from_millis(config.learner_catchup_delay_ms);

    info!(
        node_id = node_id,
        peers = ?config.seed_nodes,
        "Attempting auto-join to cluster"
    );

    // Check if we're already in the cluster
    if is_member(raft_node, node_id) {
        info!(node_id = node_id, "Already a member of the cluster");
        return Ok(false);
    }

    // Try to join with retries
    let mut last_error = None;
    for attempt in 0..=max_retries {
        if attempt > 0 {
            let delay = Duration::from_millis(500 * 2u64.pow(attempt - 1));
            debug!(
                attempt = attempt,
                delay_ms = delay.as_millis(),
                "Retrying join"
            );
            tokio::time::sleep(delay).await;
        }

        match try_join_cluster(
            &config.seed_nodes,
            node_id,
            &raft_addr,
            http_addr,
            timeout,
            learner_catchup_delay,
        )
        .await
        {
            Ok(joined) => {
                if joined {
                    info!(node_id = node_id, "Successfully joined cluster");
                } else {
                    // Already a member (race condition handled gracefully)
                    info!(node_id = node_id, "Already a member of the cluster");
                }
                return Ok(joined);
            }
            Err(e) => {
                warn!(
                    attempt = attempt,
                    error = %e,
                    "Join attempt failed"
                );
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or(ScalingError::JoinFailed("Unknown error".to_string())))
}

/// Tracks whether this node has successfully joined the cluster.
/// Used by readiness checks to determine if the node can serve requests.
pub static CLUSTER_JOINED: AtomicBool = AtomicBool::new(false);

/// Runs the auto-join worker in the background.
///
/// This function retries cluster join indefinitely with exponential backoff,
/// stopping only when: (a) join succeeds, or (b) shutdown signal is received.
///
/// # Arguments
/// * `raft_node` - The Raft node instance
/// * `config` - Cluster configuration
/// * `http_addr` - This node's HTTP address for cluster communication
/// * `shutdown_rx` - Broadcast receiver for shutdown signal
pub async fn run_auto_join_worker(
    raft_node: Arc<RaftNode>,
    config: ClusterConfig,
    http_addr: String,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    if config.seed_nodes.is_empty() {
        // Standalone cluster - mark as joined immediately
        CLUSTER_JOINED.store(true, Ordering::Release);
        return;
    }

    let node_id = config.node_id;
    info!(node_id, "Starting background auto-join worker");

    let mut attempt = 0u32;
    let base_delay = Duration::from_secs(1);
    let max_delay = Duration::from_secs(30);

    loop {
        attempt += 1;

        // Check for shutdown signal at the start of each attempt
        // This handles the case where shutdown was signaled during a join attempt
        if shutdown_rx.try_recv().is_ok() {
            info!(node_id, "Auto-join worker received shutdown signal");
            return;
        }

        // If we previously joined successfully but are no longer a member,
        // we're in the process of shutting down - don't try to re-join
        let was_joined = CLUSTER_JOINED.load(Ordering::Acquire);
        let currently_member = is_member(&raft_node, node_id);

        if currently_member {
            info!(node_id, "Already a cluster member");
            CLUSTER_JOINED.store(true, Ordering::Release);
            return;
        }

        if was_joined && !currently_member {
            // We joined before but aren't a member now - we're leaving, don't re-join
            debug!(
                node_id,
                "Previously joined but no longer a member - shutdown in progress, not re-joining"
            );
            return;
        }

        debug!(node_id, attempt, "Attempting to join cluster");

        match auto_join(&raft_node, &config, &http_addr).await {
            Ok(true) => {
                info!(node_id, "Successfully joined cluster");
                CLUSTER_JOINED.store(true, Ordering::Release);
                return;
            }
            Ok(false) => {
                // Already a member
                info!(node_id, "Already a cluster member");
                CLUSTER_JOINED.store(true, Ordering::Release);
                return;
            }
            Err(e) => {
                let delay = base_delay
                    .saturating_mul(2u32.saturating_pow(attempt.min(5) - 1))
                    .min(max_delay);

                warn!(
                    node_id,
                    attempt,
                    error = %e,
                    retry_delay_secs = delay.as_secs(),
                    "Join attempt failed, will retry"
                );

                // Wait with shutdown check
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {
                        // Continue to next attempt
                    }
                    _ = shutdown_rx.recv() => {
                        info!(node_id, "Auto-join worker received shutdown signal");
                        return;
                    }
                }
            }
        }
    }
}

/// Graceful leave on shutdown. Returns Ok(true) if left, Ok(false) if not a member.
pub async fn graceful_leave(raft_node: Arc<RaftNode>, config: &ClusterConfig) -> Result<bool> {
    let timeout = Duration::from_secs(config.leave_timeout_secs);
    let node_id = config.node_id;

    // Mark as "previously joined" to prevent auto-join worker from re-joining
    // This must happen BEFORE we remove ourselves from the cluster
    CLUSTER_JOINED.store(true, Ordering::Release);

    info!(
        node_id = node_id,
        timeout_secs = config.leave_timeout_secs,
        "Attempting graceful leave from cluster"
    );

    // Check if we're in the cluster
    if !is_member(&raft_node, node_id) {
        info!(node_id, "Not a member of the cluster, nothing to leave");
        return Ok(false);
    }

    let deadline = tokio::time::Instant::now() + timeout;

    // Check leader status
    let is_leader = raft_node.is_leader().await;
    let current_leader_id = raft_node.current_leader().await;
    let status = raft_node.get_status();
    debug!(
        node_id,
        is_leader,
        current_leader_id = ?current_leader_id,
        voters = ?status.voters,
        learners = ?status.learners,
        "Cluster status before leave"
    );

    // If we're the leader, we need special handling
    if is_leader {
        debug!("Taking leader departure path");
        info!("This node is the leader, initiating leadership transfer");

        let status = raft_node.get_status();
        if status.voters.len() <= 1 {
            // We're the only voter, just remove ourselves
            debug!("Single voter cluster, removing self directly");
            info!("Single voter cluster, removing self directly");
            if let Err(e) = raft_node.remove_node(node_id).await {
                warn!(error = %e, "Failed to remove self from single-voter cluster");
            }
            return Ok(true);
        }

        // Strategy: Remove ourselves from the voter set, triggering an election.
        // This is cleaner than demoting to learner because it directly reduces
        // the voter count, making quorum easier to achieve for remaining nodes.
        debug!(voters = ?status.voters, "Leader initiating self-removal from voters");

        // Remove ourselves from voters - this will trigger an election
        const MAX_TRANSFER_WAIT_SECS: u64 = 10;
        if let Err(e) = raft_node.remove_voters(vec![node_id]).await {
            // Critical: Don't continue if we can't remove ourselves - this leaves
            // the cluster in an inconsistent state
            return Err(ScalingError::VoterRemovalFailed(e.to_string()));
        }

        debug!("Successfully removed self from voters, waiting for new leader");

        // Wait for leadership to transfer (we should no longer be leader)
        let transfer_start = tokio::time::Instant::now();
        let max_transfer_wait = Duration::from_secs(MAX_TRANSFER_WAIT_SECS);

        while raft_node.is_leader().await {
            if transfer_start.elapsed() > max_transfer_wait {
                // Return error instead of silently continuing
                return Err(ScalingError::LeadershipTransferTimeout(
                    MAX_TRANSFER_WAIT_SECS,
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        debug!("Leadership transferred, we are now a learner");
        info!("Leadership transferred successfully");
    } else {
        debug!("Not the leader, will request removal from leader");
    }

    // Now we're not the leader, ask the leader to remove us
    let remaining_time = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining_time.is_zero() {
        return Err(ScalingError::Timeout);
    }

    // Try to get leader address and ask for removal
    match request_removal_from_leader(&raft_node, &config.seed_nodes, node_id, remaining_time).await
    {
        Ok(()) => {
            info!(node_id = node_id, "Successfully left cluster");
            Ok(true)
        }
        Err(e) => {
            warn!(error = %e, "Failed to request removal from leader");
            Err(e)
        }
    }
}

/// Checks if the given node is a member of the cluster (voter or learner).
#[must_use]
fn is_member(raft_node: &RaftNode, node_id: u64) -> bool {
    let status = raft_node.get_status();
    status.voters.contains(&node_id) || status.learners.contains(&node_id)
}

/// Creates an HTTP client with the specified timeout.
fn create_http_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .expect("Failed to create HTTP client")
}

/// Try joining via peers. Returns Ok(true) if joined, Ok(false) if already a member.
async fn try_join_cluster(
    peers: &[String],
    node_id: u64,
    raft_addr: &str,
    http_addr: &str,
    timeout: Duration,
    learner_catchup_delay: Duration,
) -> Result<bool> {
    let client = create_http_client(timeout);
    let mut connection_errors = Vec::new();

    for peer in peers {
        let peer_info = match parse_peer(peer) {
            Ok(info) => info,
            Err(e) => {
                warn!(peer = peer, error = %e, "Invalid peer format");
                continue;
            }
        };

        let base_url = peer_info.http_addr();
        debug!(url = %base_url, "Connecting to peer HTTP API");

        // Get cluster status to find the leader
        let status = match get_cluster_status(&client, &base_url).await {
            Ok(s) => s,
            Err(e) => {
                connection_errors.push(format!("{}: {}", peer_info.host, e));
                continue;
            }
        };

        // If this peer is the leader, join through it
        if status.current_leader == Some(peer_info.node_id) {
            return join_via_leader(
                &client,
                &base_url,
                node_id,
                raft_addr,
                http_addr,
                learner_catchup_delay,
            )
            .await;
        }

        // Otherwise, try to connect to the leader directly
        if let Some(leader_id) = status.current_leader {
            for other_peer in peers {
                if let Ok(other_info) = parse_peer(other_peer)
                    && other_info.node_id == leader_id
                {
                    let leader_url = other_info.http_addr();
                    return join_via_leader(
                        &client,
                        &leader_url,
                        node_id,
                        raft_addr,
                        http_addr,
                        learner_catchup_delay,
                    )
                    .await;
                }
            }
        }
    }

    if connection_errors.is_empty() {
        Err(ScalingError::NoLeader)
    } else {
        Err(ScalingError::ConnectionFailed(connection_errors.join(", ")))
    }
}

/// Gets cluster status from a peer via HTTP.
async fn get_cluster_status(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<ClusterStatusResponse> {
    let url = format!("{}/cluster/status", base_url);
    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        return Err(ScalingError::ConnectionFailed(format!(
            "Status request failed: {}",
            response.status()
        )));
    }

    response
        .json()
        .await
        .map_err(|e| ScalingError::ConnectionFailed(format!("Invalid response: {}", e)))
}

/// Join via leader. Returns Ok(true) if joined, Ok(false) if already a member.
async fn join_via_leader(
    client: &reqwest::Client,
    leader_url: &str,
    node_id: u64,
    raft_addr: &str,
    http_addr: &str,
    learner_catchup_delay: Duration,
) -> Result<bool> {
    info!(
        node_id = node_id,
        raft_addr = raft_addr,
        "Requesting to join as learner"
    );

    // Format: "node_id:host:raft_port:http_port"
    // Extract host:port components from addresses
    let raft_host_port = raft_addr.trim_start_matches("http://");
    let http_host_port = http_addr.trim_start_matches("http://");

    // The peer format expects: node_id:host:raft_port or node_id:host:raft_port:http_port
    // We need to reconstruct this from our separate addresses
    // raft_host_port is "host:raft_port", http_host_port is "host:http_port"
    // We want: "node_id:host:raft_port:http_port"
    let (host, raft_port) = raft_host_port
        .rsplit_once(':')
        .unwrap_or((raft_host_port, "9001"));
    let http_port = http_host_port
        .rsplit_once(':')
        .map(|(_, p)| p)
        .unwrap_or("9000");

    let node_spec = format!("{}:{}:{}:{}", node_id, host, raft_port, http_port);

    let url = format!("{}/cluster/members", leader_url);
    let response = client
        .post(&url)
        .json(&AddLearnerRequest { node: node_spec })
        .send()
        .await?;

    let resp: MembershipResponse = response
        .json()
        .await
        .map_err(|e| ScalingError::JoinFailed(format!("Invalid response from leader: {}", e)))?;

    if !resp.success {
        // Handle "already exists" gracefully - not an error
        let msg_lower = resp.message.to_lowercase();
        if msg_lower.contains("already") || msg_lower.contains("exists") {
            return Ok(false);
        }
        return Err(ScalingError::JoinFailed(resp.message));
    }

    info!(node_id = node_id, "Added as learner");

    // Request promotion to voter after catching up
    tokio::time::sleep(learner_catchup_delay).await;

    debug!(node_id = node_id, "Requesting promotion to voter");
    let url = format!("{}/cluster/members/promote", leader_url);
    let response = client
        .post(&url)
        .json(&PromoteVotersRequest {
            node_ids: vec![node_id],
        })
        .send()
        .await?;

    let resp: MembershipResponse = response
        .json()
        .await
        .map_err(|e| ScalingError::JoinFailed(format!("Invalid promote response: {}", e)))?;

    if !resp.success {
        warn!(
            node_id = node_id,
            error = resp.message,
            "Promotion to voter failed, remaining as learner"
        );
        // Don't fail completely - being a learner is still progress
    } else {
        info!(node_id = node_id, "Promoted to voter");
    }

    Ok(true)
}

/// Request removal via leader. Retries with backoff during leader elections.
async fn request_removal_from_leader(
    raft_node: &RaftNode,
    peers: &[String],
    node_id: u64,
    timeout: Duration,
) -> Result<()> {
    debug!(node_id, peers = ?peers, "Requesting removal from leader");

    let deadline = tokio::time::Instant::now() + timeout;
    let mut attempt = 0u32;
    let base_delay = Duration::from_millis(500);
    let max_delay = Duration::from_secs(5);

    // Create HTTP client once with full timeout - reuse across retries
    let client = create_http_client(timeout);

    loop {
        attempt += 1;
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            debug!(attempt, "Timeout waiting for leader");
            return Err(ScalingError::Timeout);
        }

        debug!(
            attempt,
            remaining_ms = remaining.as_millis(),
            "Attempting to find leader for removal"
        );

        // First try to get leader info from our local raft node
        let leader_id = raft_node.current_leader().await;
        debug!(leader_id = ?leader_id, attempt, "Current leader from local raft node");

        if let Some(leader_id) = leader_id {
            for peer in peers {
                if let Ok(info) = parse_peer(peer)
                    && info.node_id == leader_id
                {
                    let leader_url = info.http_addr();
                    debug!(leader_id, leader_url = %leader_url, "Found leader in peers, attempting removal");
                    match remove_via_http(&client, &leader_url, node_id).await {
                        Ok(()) => return Ok(()),
                        Err(e) => {
                            debug!(error = %e, "Remove via leader failed, will retry");
                        }
                    }
                }
            }
            debug!("Leader not found in peers list, will try fallback");
        }

        // Fallback: try each peer to find the leader
        let mut found_any_leader = false;
        debug!("Trying fallback - contacting each peer to find leader");
        for peer in peers {
            let peer_info = match parse_peer(peer) {
                Ok(info) => info,
                Err(e) => {
                    debug!(peer, error = %e, "Failed to parse peer");
                    continue;
                }
            };

            let base_url = peer_info.http_addr();

            // Get status to check if this is the leader
            let status = match get_cluster_status(&client, &base_url).await {
                Ok(s) => {
                    debug!(url = %base_url, leader = ?s.current_leader, "Got cluster status from peer");
                    s
                }
                Err(e) => {
                    debug!(url = %base_url, error = %e, "Failed to get status from peer");
                    continue;
                }
            };

            // If this peer reports a leader, try to contact them
            if let Some(reported_leader_id) = status.current_leader {
                found_any_leader = true;

                // If this peer IS the leader, use them directly
                if reported_leader_id == peer_info.node_id {
                    debug!(leader_url = %base_url, "Peer is the leader, requesting removal");
                    match remove_via_http(&client, &base_url, node_id).await {
                        Ok(()) => return Ok(()),
                        Err(e) => {
                            warn!(peer = peer, error = %e, "Failed to remove via leader");
                        }
                    }
                } else {
                    // This peer reports a different leader, try to find that leader in our peer list
                    debug!(peer_url = %base_url, reported_leader = reported_leader_id, "Peer reports different leader");
                    for other_peer in peers {
                        if let Ok(other_info) = parse_peer(other_peer)
                            && other_info.node_id == reported_leader_id
                        {
                            let leader_url = other_info.http_addr();
                            debug!(leader_id = reported_leader_id, leader_url = %leader_url, "Found reported leader, requesting removal");
                            match remove_via_http(&client, &leader_url, node_id).await {
                                Ok(()) => return Ok(()),
                                Err(e) => {
                                    warn!(leader_id = reported_leader_id, error = %e, "Failed to remove via reported leader");
                                }
                            }
                            break;
                        }
                    }
                }
            } else {
                debug!(url = %base_url, "Peer reports no leader");
            }
        }

        // No leader found, wait and retry with exponential backoff
        if !found_any_leader {
            let delay = base_delay
                .saturating_mul(2u32.saturating_pow(attempt.min(5) - 1))
                .min(max_delay);
            let delay = delay.min(remaining);
            debug!(
                attempt,
                delay_ms = delay.as_millis(),
                "No leader found, waiting before retry"
            );
            tokio::time::sleep(delay).await;
        }
    }
}

/// Removes this node from the cluster via HTTP API.
async fn remove_via_http(client: &reqwest::Client, base_url: &str, node_id: u64) -> Result<()> {
    let url = format!("{}/cluster/members/{}", base_url, node_id);
    debug!(url = %url, "Sending DELETE request to remove node");

    let response = client.delete(&url).send().await?;
    let status = response.status();

    let resp: MembershipResponse = response
        .json()
        .await
        .map_err(|e| ScalingError::LeaveFailed(format!("Invalid response: {}", e)))?;

    debug!(
        status = %status,
        success = resp.success,
        message = %resp.message,
        "Remove node response"
    );

    if resp.success {
        Ok(())
    } else {
        Err(ScalingError::LeaveFailed(resp.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scaling_error_display() {
        // Verify error messages are descriptive (not empty or generic)
        assert_eq!(ScalingError::NoPeers.to_string(), "No peers configured");
        assert_eq!(
            ScalingError::NoLeader.to_string(),
            "No leader available in cluster"
        );
        assert_eq!(ScalingError::Timeout.to_string(), "Operation timed out");

        // Verify parameterized errors include their context
        assert!(
            ScalingError::JoinFailed("test".into())
                .to_string()
                .contains("test")
        );
        assert!(
            ScalingError::LeaveFailed("test".into())
                .to_string()
                .contains("test")
        );
        assert!(
            ScalingError::LeadershipTransferTimeout(10)
                .to_string()
                .contains("10")
        );
        assert!(
            ScalingError::VoterRemovalFailed("test".into())
                .to_string()
                .contains("test")
        );
    }
}
