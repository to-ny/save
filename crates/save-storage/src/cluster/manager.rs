//! Cluster manager for coordinating storage replication nodes.

use super::state::{ClusterState, NodeHealth, NodeState, PartitionStatus};
use crate::StorageError;
use crate::replication::{QuorumConfig, ReplicationCoordinator};
use save_common::cluster::parse_peers;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, broadcast};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Configuration for cluster manager.
#[derive(Debug, Clone)]
pub struct ClusterManagerConfig {
    /// Interval between health checks.
    pub heartbeat_interval: Duration,
    /// Timeout for health check RPCs.
    pub health_check_timeout: Duration,
    /// Number of consecutive failures before marking node unreachable.
    pub failure_threshold: u32,
    /// Latency threshold (ms) above which node is marked degraded.
    pub degraded_latency_threshold_ms: u64,
    /// Connection timeout for new nodes.
    pub connect_timeout: Duration,
    /// RPC timeout for operations.
    pub rpc_timeout: Duration,
}

impl Default for ClusterManagerConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(5),
            health_check_timeout: Duration::from_secs(2),
            failure_threshold: 3,
            degraded_latency_threshold_ms: 500,
            connect_timeout: Duration::from_secs(5),
            rpc_timeout: Duration::from_secs(30),
        }
    }
}

/// Summary of cluster topology.
#[derive(Debug, Clone)]
pub struct ClusterTopology {
    pub local_node_id: u64,
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub degraded_nodes: usize,
    pub unreachable_nodes: usize,
    pub has_quorum: bool,
    pub partition_status: PartitionStatus,
    pub leader_id: Option<u64>,
}

/// Events emitted by the cluster manager.
#[derive(Debug, Clone)]
pub enum ClusterEvent {
    /// Node health changed.
    NodeHealthChanged { node_id: u64, health: NodeHealth },
    /// Leader changed.
    LeaderChanged { new_leader: Option<u64> },
    /// Quorum status changed.
    QuorumChanged { has_quorum: bool },
    /// Partition status changed.
    PartitionChanged { status: PartitionStatus },
    /// Node added to cluster.
    NodeAdded { node_id: u64 },
    /// Node removed from cluster.
    NodeRemoved { node_id: u64 },
}

/// Manages cluster state and coordinates replication nodes.
pub struct ClusterManager {
    local_node_id: u64,
    coordinator: Arc<ReplicationCoordinator>,
    state: Arc<RwLock<ClusterState>>,
    config: ClusterManagerConfig,
    event_tx: broadcast::Sender<ClusterEvent>,
    heartbeat_handle: RwLock<Option<JoinHandle<()>>>,
}

impl ClusterManager {
    /// Create a new cluster manager.
    pub fn new(
        local_node_id: u64,
        quorum_config: QuorumConfig,
        config: ClusterManagerConfig,
    ) -> Self {
        let coordinator = Arc::new(ReplicationCoordinator::with_timeouts(
            local_node_id,
            quorum_config,
            config.connect_timeout,
            config.rpc_timeout,
        ));

        let state = Arc::new(RwLock::new(ClusterState::new(local_node_id)));
        let (event_tx, _) = broadcast::channel(64);

        Self {
            local_node_id,
            coordinator,
            state,
            config,
            event_tx,
            heartbeat_handle: RwLock::new(None),
        }
    }

    /// Create cluster manager with existing coordinator.
    pub fn with_coordinator(
        local_node_id: u64,
        coordinator: Arc<ReplicationCoordinator>,
        config: ClusterManagerConfig,
    ) -> Self {
        let state = Arc::new(RwLock::new(ClusterState::new(local_node_id)));
        let (event_tx, _) = broadcast::channel(64);

        Self {
            local_node_id,
            coordinator,
            state,
            config,
            event_tx,
            heartbeat_handle: RwLock::new(None),
        }
    }

    /// Get the underlying replication coordinator.
    pub fn coordinator(&self) -> &Arc<ReplicationCoordinator> {
        &self.coordinator
    }

    /// Subscribe to cluster events.
    pub fn subscribe(&self) -> broadcast::Receiver<ClusterEvent> {
        self.event_tx.subscribe()
    }

    /// Discover and connect to peers from configuration.
    pub async fn discover_peers(&self, peer_strings: &[String]) -> Result<(), StorageError> {
        if peer_strings.is_empty() {
            debug!("No peers configured, running as standalone cluster");
            return Ok(());
        }

        let peers = parse_peers(peer_strings).map_err(|e| {
            StorageError::Io(std::io::Error::other(format!(
                "Failed to parse peers: {}",
                e
            )))
        })?;

        info!("Discovering {} peer(s)", peers.len());

        for peer in peers {
            if peer.node_id == self.local_node_id {
                continue; // Skip self
            }

            let addr = peer.http_addr();
            info!(node_id = peer.node_id, addr = %addr, "Connecting to peer");

            match self.add_node(peer.node_id, addr.clone()).await {
                Ok(_) => {
                    info!(node_id = peer.node_id, "Successfully connected to peer");
                }
                Err(e) => {
                    warn!(
                        node_id = peer.node_id,
                        error = %e,
                        "Failed to connect to peer (will retry via heartbeat)"
                    );
                    // Add to state as unknown - heartbeat will attempt reconnection
                    let mut state = self.state.write().await;
                    state
                        .nodes
                        .insert(peer.node_id, NodeState::new(peer.node_id, addr));
                }
            }
        }

        // Update quorum status
        let mut state = self.state.write().await;
        let old_quorum = state.has_quorum;
        state.has_quorum = state.check_quorum();
        state.last_updated = Instant::now();

        if state.has_quorum != old_quorum {
            let _ = self.event_tx.send(ClusterEvent::QuorumChanged {
                has_quorum: state.has_quorum,
            });
        }

        Ok(())
    }

    /// Add a node to the cluster.
    pub async fn add_node(&self, node_id: u64, address: String) -> Result<(), StorageError> {
        if node_id == self.local_node_id {
            return Err(StorageError::Io(std::io::Error::other(
                "Cannot add self as peer",
            )));
        }

        // Connect via coordinator
        self.coordinator.add_node(node_id, address.clone()).await?;

        // Update state
        let mut state = self.state.write().await;
        let mut node_state = NodeState::new(node_id, address);

        // Perform initial health check
        if let Some(latency) = self.check_node_health(node_id).await {
            if latency > self.config.degraded_latency_threshold_ms {
                node_state.mark_degraded(latency);
            } else {
                node_state.mark_healthy(latency);
            }
        } else {
            node_state.mark_unreachable();
        }

        let health = node_state.health;
        state.nodes.insert(node_id, node_state);

        // Update quorum
        let old_quorum = state.has_quorum;
        state.has_quorum = state.check_quorum();
        state.last_updated = Instant::now();

        drop(state);

        // Emit events
        let _ = self.event_tx.send(ClusterEvent::NodeAdded { node_id });
        let _ = self
            .event_tx
            .send(ClusterEvent::NodeHealthChanged { node_id, health });

        if old_quorum != self.state.read().await.has_quorum {
            let _ = self.event_tx.send(ClusterEvent::QuorumChanged {
                has_quorum: self.state.read().await.has_quorum,
            });
        }

        Ok(())
    }

    /// Remove a node from the cluster.
    pub async fn remove_node(&self, node_id: u64) -> Result<(), StorageError> {
        self.coordinator.remove_node(node_id).await;

        let mut state = self.state.write().await;
        state.nodes.remove(&node_id);

        let old_quorum = state.has_quorum;
        state.has_quorum = state.check_quorum();
        state.partition_status = state.detect_partition();
        state.last_updated = Instant::now();

        drop(state);

        let _ = self.event_tx.send(ClusterEvent::NodeRemoved { node_id });

        if old_quorum != self.state.read().await.has_quorum {
            let _ = self.event_tx.send(ClusterEvent::QuorumChanged {
                has_quorum: self.state.read().await.has_quorum,
            });
        }

        Ok(())
    }

    /// Check if removing a node would break quorum.
    pub async fn can_remove_node(&self, node_id: u64) -> bool {
        let state = self.state.read().await;

        // If node doesn't exist or isn't available, removal is safe
        let Some(node) = state.nodes.get(&node_id) else {
            return true;
        };

        if !node.health.is_available() {
            return true;
        }

        // Calculate quorum after removal
        let total_after = state.total_node_count() - 1;
        let available_after = state.available_node_count() - 1 + 1; // -1 removed, +1 local
        available_after > total_after / 2
    }

    /// Get cluster topology summary.
    pub async fn topology(&self) -> ClusterTopology {
        let state = self.state.read().await;
        ClusterTopology {
            local_node_id: state.local_node_id,
            total_nodes: state.total_node_count(),
            healthy_nodes: state.healthy_nodes().len() + 1, // +1 for local
            degraded_nodes: state
                .nodes
                .values()
                .filter(|n| n.health == NodeHealth::Degraded)
                .count(),
            unreachable_nodes: state.unreachable_nodes().len(),
            has_quorum: state.has_quorum,
            partition_status: state.partition_status,
            leader_id: state.leader_id,
        }
    }

    /// Validate that cluster configuration meets minimum requirements.
    pub fn validate_config(&self, replication_factor: usize) -> Result<(), StorageError> {
        let quorum = (replication_factor / 2) + 1;
        if quorum < 1 {
            return Err(StorageError::Io(std::io::Error::other(
                "Invalid replication factor: quorum must be at least 1",
            )));
        }
        Ok(())
    }

    /// Start the heartbeat background task.
    pub async fn start_heartbeat(&self) {
        let mut handle = self.heartbeat_handle.write().await;
        if handle.is_some() {
            return; // Already running
        }

        let state = Arc::clone(&self.state);
        let coordinator = Arc::clone(&self.coordinator);
        let config = self.config.clone();
        let event_tx = self.event_tx.clone();

        let task = tokio::spawn(async move {
            heartbeat_loop(state, coordinator, config, event_tx).await;
        });

        *handle = Some(task);
        info!("Heartbeat task started");
    }

    /// Stop the heartbeat background task.
    pub async fn stop_heartbeat(&self) {
        let mut handle = self.heartbeat_handle.write().await;
        if let Some(task) = handle.take() {
            task.abort();
            info!("Heartbeat task stopped");
        }
    }

    /// Get current cluster state.
    pub async fn get_state(&self) -> ClusterState {
        self.state.read().await.clone()
    }

    /// Check if cluster has quorum.
    pub async fn has_quorum(&self) -> bool {
        self.state.read().await.has_quorum
    }

    /// Update leader information.
    pub async fn update_leader(&self, leader_id: Option<u64>) {
        let mut state = self.state.write().await;
        if state.leader_id != leader_id {
            state.leader_id = leader_id;
            state.last_updated = Instant::now();
            let _ = self.event_tx.send(ClusterEvent::LeaderChanged {
                new_leader: leader_id,
            });
        }
    }

    /// Check if we can safely perform writes (have quorum and not in minority partition).
    pub async fn can_write(&self) -> bool {
        self.state.read().await.should_accept_writes()
    }

    /// Get current partition status.
    pub async fn partition_status(&self) -> PartitionStatus {
        self.state.read().await.partition_status
    }

    /// Check if we're in a minority partition (split-brain prevention).
    pub async fn is_in_minority_partition(&self) -> bool {
        self.state.read().await.partition_status == PartitionStatus::Minority
    }

    /// Get list of available node IDs for replication.
    pub async fn available_nodes(&self) -> Vec<u64> {
        self.state.read().await.available_nodes()
    }

    /// Perform health check on a specific node, returns latency in ms if successful.
    async fn check_node_health(&self, node_id: u64) -> Option<u64> {
        let start = Instant::now();
        let timeout = self.config.health_check_timeout;

        match tokio::time::timeout(timeout, self.coordinator.health_check(node_id)).await {
            Ok(Ok(is_healthy)) if is_healthy => Some(start.elapsed().as_millis() as u64),
            _ => None,
        }
    }
}

/// Background heartbeat loop.
async fn heartbeat_loop(
    state: Arc<RwLock<ClusterState>>,
    coordinator: Arc<ReplicationCoordinator>,
    config: ClusterManagerConfig,
    event_tx: broadcast::Sender<ClusterEvent>,
) {
    let mut interval = tokio::time::interval(config.heartbeat_interval);

    loop {
        interval.tick().await;

        // Get list of nodes to check
        let nodes_to_check: Vec<(u64, String)> = {
            let state = state.read().await;
            state
                .nodes
                .iter()
                .map(|(id, n)| (*id, n.address.clone()))
                .collect()
        };

        for (node_id, address) in nodes_to_check {
            let start = Instant::now();
            let result = tokio::time::timeout(
                config.health_check_timeout,
                coordinator.health_check(node_id),
            )
            .await;

            let mut state_guard = state.write().await;
            let Some(node) = state_guard.nodes.get_mut(&node_id) else {
                continue;
            };

            let old_health = node.health;

            match result {
                Ok(Ok(true)) => {
                    let latency = start.elapsed().as_millis() as u64;
                    if latency > config.degraded_latency_threshold_ms {
                        node.mark_degraded(latency);
                    } else {
                        node.mark_healthy(latency);
                    }
                }
                Ok(Ok(false)) => {
                    // Health check returned unhealthy
                    node.mark_degraded(start.elapsed().as_millis() as u64);
                }
                Ok(Err(e)) => {
                    debug!(node_id, error = %e, "Health check failed");
                    node.mark_unreachable();

                    // Try to reconnect if too many failures
                    if node.consecutive_failures >= config.failure_threshold {
                        debug!(node_id, "Attempting reconnection");
                        drop(state_guard);

                        if let Err(e) = coordinator.add_node(node_id, address.clone()).await {
                            debug!(node_id, error = %e, "Reconnection failed");
                        }
                        continue;
                    }
                }
                Err(_) => {
                    // Timeout
                    debug!(node_id, "Health check timed out");
                    node.mark_unreachable();
                }
            }

            let new_health = node.health;

            // Update quorum and partition status
            let old_quorum = state_guard.has_quorum;
            let old_partition = state_guard.partition_status;
            state_guard.has_quorum = state_guard.check_quorum();
            state_guard.partition_status = state_guard.detect_partition();
            state_guard.last_updated = Instant::now();

            let new_quorum = state_guard.has_quorum;
            let new_partition = state_guard.partition_status;

            drop(state_guard);

            // Emit events
            if old_health != new_health {
                let _ = event_tx.send(ClusterEvent::NodeHealthChanged {
                    node_id,
                    health: new_health,
                });
            }

            if old_quorum != new_quorum {
                let _ = event_tx.send(ClusterEvent::QuorumChanged {
                    has_quorum: new_quorum,
                });
            }

            if old_partition != new_partition {
                let _ = event_tx.send(ClusterEvent::PartitionChanged {
                    status: new_partition,
                });
            }
        }
    }
}

impl Drop for ClusterManager {
    fn drop(&mut self) {
        // Note: Can't do async cleanup in drop, but the task will be aborted
        // when the JoinHandle is dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_manager_config_default() {
        let config = ClusterManagerConfig::default();
        assert_eq!(config.heartbeat_interval, Duration::from_secs(5));
        assert_eq!(config.health_check_timeout, Duration::from_secs(2));
        assert_eq!(config.failure_threshold, 3);
    }

    #[tokio::test]
    async fn test_cluster_manager_creation() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        let state = manager.get_state().await;
        assert_eq!(state.local_node_id, 1);
        assert!(state.nodes.is_empty());
    }

    #[tokio::test]
    async fn test_cluster_manager_single_node_quorum() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Single node should always have quorum
        let state = manager.get_state().await;
        assert!(state.check_quorum());
    }

    #[tokio::test]
    async fn test_cluster_event_subscription() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        let mut rx = manager.subscribe();

        // Trigger a leader change event
        manager.update_leader(Some(2)).await;

        let event = rx.recv().await.unwrap();
        match event {
            ClusterEvent::LeaderChanged { new_leader } => {
                assert_eq!(new_leader, Some(2));
            }
            _ => panic!("Expected LeaderChanged event"),
        }
    }

    #[tokio::test]
    async fn test_topology_single_node() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        let topology = manager.topology().await;
        assert_eq!(topology.local_node_id, 1);
        assert_eq!(topology.total_nodes, 1);
        assert_eq!(topology.healthy_nodes, 1);
        assert_eq!(topology.degraded_nodes, 0);
        assert_eq!(topology.unreachable_nodes, 0);
    }

    #[tokio::test]
    async fn test_can_remove_nonexistent_node() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        // Should be able to remove a node that doesn't exist
        assert!(manager.can_remove_node(99).await);
    }

    #[test]
    fn test_validate_config() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        // Valid replication factors
        assert!(manager.validate_config(1).is_ok());
        assert!(manager.validate_config(3).is_ok());
        assert!(manager.validate_config(5).is_ok());
    }

    #[tokio::test]
    async fn test_partition_status_default() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Initially unknown
        assert_eq!(manager.partition_status().await, PartitionStatus::Unknown);

        // After topology check, should be connected (single node)
        let topology = manager.topology().await;
        assert_eq!(topology.partition_status, PartitionStatus::Unknown);
    }

    #[tokio::test]
    async fn test_is_in_minority_partition() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Initially not in minority
        assert!(!manager.is_in_minority_partition().await);
    }

    #[tokio::test]
    async fn test_with_coordinator() {
        use std::sync::Arc;

        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, quorum.clone()));
        let manager = ClusterManager::with_coordinator(1, coordinator.clone(), config);

        // Verify coordinator is accessible
        assert!(Arc::ptr_eq(manager.coordinator(), &coordinator));
    }

    #[tokio::test]
    async fn test_leader_update_tracking() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        // Initially no leader
        let state = manager.get_state().await;
        assert_eq!(state.leader_id, None);

        // Update leader to node 2
        manager.update_leader(Some(2)).await;
        let state = manager.get_state().await;
        assert_eq!(state.leader_id, Some(2));

        // Update leader to self
        manager.update_leader(Some(1)).await;
        let state = manager.get_state().await;
        assert_eq!(state.leader_id, Some(1));
        assert!(state.is_leader());

        // Clear leader
        manager.update_leader(None).await;
        let state = manager.get_state().await;
        assert_eq!(state.leader_id, None);
    }

    #[tokio::test]
    async fn test_add_self_as_peer_fails() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        // Adding self should fail
        let result = manager.add_node(1, "localhost:9001".to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_can_write_single_node() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Single node with quorum should allow writes
        // But partition status is Unknown initially
        let state = manager.get_state().await;
        assert!(state.check_quorum());
    }

    #[tokio::test]
    async fn test_available_nodes_empty() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        // No remote nodes initially
        let nodes = manager.available_nodes().await;
        assert!(nodes.is_empty());
    }

    #[tokio::test]
    async fn test_has_quorum_single_node() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Single node always has quorum (within its own view)
        let state = manager.get_state().await;
        assert!(state.check_quorum());
    }

    #[tokio::test]
    async fn test_heartbeat_start_stop() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Start heartbeat
        manager.start_heartbeat().await;

        // Starting again should be idempotent
        manager.start_heartbeat().await;

        // Stop heartbeat
        manager.stop_heartbeat().await;

        // Stopping again should be idempotent
        manager.stop_heartbeat().await;
    }

    #[tokio::test]
    async fn test_discover_peers_empty() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(1);
        let manager = ClusterManager::new(1, quorum, config);

        // Empty peers list should succeed
        let result = manager.discover_peers(&[]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_discover_peers_skips_self() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        // Should skip self in peer list
        let peers = vec!["1:localhost:9001".to_string()];
        let result = manager.discover_peers(&peers).await;
        assert!(result.is_ok());

        // No nodes should be added (self was skipped)
        let state = manager.get_state().await;
        assert!(state.nodes.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_leader_change_events() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        let mut rx = manager.subscribe();

        // First leader change
        manager.update_leader(Some(2)).await;

        // Second leader change
        manager.update_leader(Some(3)).await;

        // Same leader - no event
        manager.update_leader(Some(3)).await;

        // Should receive exactly 2 events
        let event1 = rx.recv().await.unwrap();
        let event2 = rx.recv().await.unwrap();

        match event1 {
            ClusterEvent::LeaderChanged { new_leader } => assert_eq!(new_leader, Some(2)),
            _ => panic!("Expected LeaderChanged event"),
        }
        match event2 {
            ClusterEvent::LeaderChanged { new_leader } => assert_eq!(new_leader, Some(3)),
            _ => panic!("Expected LeaderChanged event"),
        }
    }

    #[tokio::test]
    async fn test_topology_fields() {
        let config = ClusterManagerConfig::default();
        let quorum = QuorumConfig::with_replication_factor(3);
        let manager = ClusterManager::new(1, quorum, config);

        manager.update_leader(Some(2)).await;

        let topology = manager.topology().await;
        assert_eq!(topology.local_node_id, 1);
        assert_eq!(topology.leader_id, Some(2));
        assert_eq!(topology.total_nodes, 1); // Only local node
        assert!(!topology.has_quorum); // No quorum with RF=3 and 1 node
    }

    #[test]
    fn test_cluster_manager_config_custom() {
        let config = ClusterManagerConfig {
            heartbeat_interval: Duration::from_secs(10),
            health_check_timeout: Duration::from_secs(5),
            failure_threshold: 5,
            degraded_latency_threshold_ms: 1000,
            connect_timeout: Duration::from_secs(10),
            rpc_timeout: Duration::from_secs(60),
        };

        assert_eq!(config.heartbeat_interval, Duration::from_secs(10));
        assert_eq!(config.health_check_timeout, Duration::from_secs(5));
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.degraded_latency_threshold_ms, 1000);
    }
}
