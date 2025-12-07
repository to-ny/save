//! Cluster state tracking types.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Node health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeHealth {
    /// Node is responding normally.
    Healthy,
    /// Node is responding but degraded (e.g., high latency, disk issues).
    Degraded,
    /// Node is not responding to health checks.
    Unreachable,
    /// Node health is not yet known (initial state).
    Unknown,
}

/// Partition status for split-brain detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionStatus {
    /// Cluster is fully connected.
    Connected,
    /// This node may be in a minority partition (cannot reach majority).
    PossibleMinority,
    /// This node is definitely in a minority partition.
    Minority,
    /// Partition status unknown (not enough information).
    Unknown,
}

impl NodeHealth {
    pub fn is_available(&self) -> bool {
        matches!(self, NodeHealth::Healthy | NodeHealth::Degraded)
    }
}

/// State of a single node in the cluster.
#[derive(Debug, Clone)]
pub struct NodeState {
    pub node_id: u64,
    pub address: String,
    pub health: NodeHealth,
    pub last_seen: Option<Instant>,
    pub last_check: Option<Instant>,
    pub consecutive_failures: u32,
    pub latency_ms: Option<u64>,
}

impl NodeState {
    pub fn new(node_id: u64, address: String) -> Self {
        Self {
            node_id,
            address,
            health: NodeHealth::Unknown,
            last_seen: None,
            last_check: None,
            consecutive_failures: 0,
            latency_ms: None,
        }
    }

    pub fn mark_healthy(&mut self, latency_ms: u64) {
        self.health = NodeHealth::Healthy;
        self.last_seen = Some(Instant::now());
        self.last_check = Some(Instant::now());
        self.consecutive_failures = 0;
        self.latency_ms = Some(latency_ms);
    }

    pub fn mark_degraded(&mut self, latency_ms: u64) {
        self.health = NodeHealth::Degraded;
        self.last_seen = Some(Instant::now());
        self.last_check = Some(Instant::now());
        self.consecutive_failures = 0;
        self.latency_ms = Some(latency_ms);
    }

    pub fn mark_unreachable(&mut self) {
        self.health = NodeHealth::Unreachable;
        self.last_check = Some(Instant::now());
        self.consecutive_failures += 1;
        self.latency_ms = None;
    }
}

/// Overall cluster state.
#[derive(Debug, Clone)]
pub struct ClusterState {
    pub local_node_id: u64,
    pub nodes: HashMap<u64, NodeState>,
    pub leader_id: Option<u64>,
    pub has_quorum: bool,
    pub partition_status: PartitionStatus,
    pub last_updated: Instant,
}

impl ClusterState {
    pub fn new(local_node_id: u64) -> Self {
        Self {
            local_node_id,
            nodes: HashMap::new(),
            leader_id: None,
            has_quorum: false,
            partition_status: PartitionStatus::Unknown,
            last_updated: Instant::now(),
        }
    }

    /// Count of nodes that are available (healthy or degraded).
    pub fn available_node_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.health.is_available())
            .count()
    }

    /// Count of total known nodes (including self).
    pub fn total_node_count(&self) -> usize {
        self.nodes.len() + 1 // +1 for local node
    }

    /// Check if we have quorum (majority of nodes available).
    pub fn check_quorum(&self) -> bool {
        let total = self.total_node_count();
        let available = self.available_node_count() + 1; // +1 for local (always available)
        available > total / 2
    }

    /// Get all healthy node IDs.
    pub fn healthy_nodes(&self) -> Vec<u64> {
        self.nodes
            .values()
            .filter(|n| n.health == NodeHealth::Healthy)
            .map(|n| n.node_id)
            .collect()
    }

    /// Get all available node IDs (healthy or degraded).
    pub fn available_nodes(&self) -> Vec<u64> {
        self.nodes
            .values()
            .filter(|n| n.health.is_available())
            .map(|n| n.node_id)
            .collect()
    }

    /// Get unreachable node IDs.
    pub fn unreachable_nodes(&self) -> Vec<u64> {
        self.nodes
            .values()
            .filter(|n| n.health == NodeHealth::Unreachable)
            .map(|n| n.node_id)
            .collect()
    }

    /// Detect partition status based on node reachability.
    pub fn detect_partition(&self) -> PartitionStatus {
        let total = self.total_node_count();
        let unreachable = self
            .nodes
            .values()
            .filter(|n| !n.health.is_available())
            .count();

        if unreachable == 0 {
            PartitionStatus::Connected
        } else if self.check_quorum() {
            // We can reach majority, likely in the larger partition
            PartitionStatus::Connected
        } else {
            // Cannot reach majority - we're in a minority partition
            if total > 1 {
                PartitionStatus::Minority
            } else {
                PartitionStatus::Unknown
            }
        }
    }

    /// Check if we're potentially in a minority partition.
    /// Uses recent failure patterns to detect asymmetric partitions.
    pub fn is_possibly_partitioned(&self, stale_threshold: Duration) -> bool {
        let now = Instant::now();
        self.nodes.values().any(|n| {
            n.health == NodeHealth::Unreachable
                && n.last_seen
                    .is_some_and(|t| now.duration_since(t) > stale_threshold)
        })
    }

    /// Check if writes should be accepted (quorum + not in minority partition).
    pub fn should_accept_writes(&self) -> bool {
        self.has_quorum && self.partition_status != PartitionStatus::Minority
    }

    /// Check if this node might be the leader based on available information.
    pub fn is_leader(&self) -> bool {
        self.leader_id == Some(self.local_node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_health_is_available() {
        assert!(NodeHealth::Healthy.is_available());
        assert!(NodeHealth::Degraded.is_available());
        assert!(!NodeHealth::Unreachable.is_available());
        assert!(!NodeHealth::Unknown.is_available());
    }

    #[test]
    fn test_node_state_transitions() {
        let mut node = NodeState::new(1, "localhost:9001".to_string());
        assert_eq!(node.health, NodeHealth::Unknown);
        assert_eq!(node.consecutive_failures, 0);

        node.mark_healthy(10);
        assert_eq!(node.health, NodeHealth::Healthy);
        assert!(node.last_seen.is_some());
        assert_eq!(node.latency_ms, Some(10));

        node.mark_unreachable();
        assert_eq!(node.health, NodeHealth::Unreachable);
        assert_eq!(node.consecutive_failures, 1);

        node.mark_unreachable();
        assert_eq!(node.consecutive_failures, 2);

        node.mark_degraded(50);
        assert_eq!(node.health, NodeHealth::Degraded);
        assert_eq!(node.consecutive_failures, 0);
    }

    #[test]
    fn test_cluster_state_quorum() {
        let mut state = ClusterState::new(1);

        // Single node - always has quorum
        assert!(state.check_quorum());

        // Add 2 more nodes (3 total), both healthy
        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_healthy(5);
            n
        });
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_healthy(5);
            n
        });

        // 3 nodes, all available (local + 2 remote) = quorum
        assert!(state.check_quorum());

        // Make one unreachable - still have quorum (2/3)
        state.nodes.get_mut(&2).unwrap().mark_unreachable();
        assert!(state.check_quorum());

        // Make another unreachable - lost quorum (1/3)
        state.nodes.get_mut(&3).unwrap().mark_unreachable();
        assert!(!state.check_quorum());
    }

    #[test]
    fn test_cluster_state_node_lists() {
        let mut state = ClusterState::new(1);

        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_healthy(5);
            n
        });
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_degraded(100);
            n
        });
        state.nodes.insert(4, {
            let mut n = NodeState::new(4, "host4:9001".to_string());
            n.mark_unreachable();
            n
        });

        assert_eq!(state.healthy_nodes(), vec![2]);
        assert_eq!(state.available_node_count(), 2);
        assert_eq!(state.unreachable_nodes(), vec![4]);

        let available = state.available_nodes();
        assert!(available.contains(&2));
        assert!(available.contains(&3));
        assert!(!available.contains(&4));
    }

    #[test]
    fn test_partition_detection_connected() {
        let mut state = ClusterState::new(1);

        // Single node - connected
        assert_eq!(state.detect_partition(), PartitionStatus::Connected);

        // Add healthy nodes
        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_healthy(5);
            n
        });
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_healthy(5);
            n
        });

        assert_eq!(state.detect_partition(), PartitionStatus::Connected);
    }

    #[test]
    fn test_partition_detection_minority() {
        let mut state = ClusterState::new(1);

        // 3-node cluster where we can only reach ourselves
        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_unreachable();
            n
        });
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_unreachable();
            n
        });

        // Can't reach majority - in minority partition
        assert_eq!(state.detect_partition(), PartitionStatus::Minority);
    }

    #[test]
    fn test_partition_detection_with_quorum() {
        let mut state = ClusterState::new(1);

        // 3-node cluster where we can reach one other node
        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_healthy(5);
            n
        });
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_unreachable();
            n
        });

        // Can reach majority (2/3) - considered connected
        assert_eq!(state.detect_partition(), PartitionStatus::Connected);
    }

    #[test]
    fn test_should_accept_writes() {
        let mut state = ClusterState::new(1);
        state.has_quorum = true;
        state.partition_status = PartitionStatus::Connected;

        assert!(state.should_accept_writes());

        // No quorum - reject writes
        state.has_quorum = false;
        assert!(!state.should_accept_writes());

        // Has quorum but in minority partition - reject writes
        state.has_quorum = true;
        state.partition_status = PartitionStatus::Minority;
        assert!(!state.should_accept_writes());
    }

    #[test]
    fn test_is_leader() {
        let mut state = ClusterState::new(1);

        assert!(!state.is_leader()); // No leader set

        state.leader_id = Some(1);
        assert!(state.is_leader()); // We are the leader

        state.leader_id = Some(2);
        assert!(!state.is_leader()); // Someone else is leader
    }

    #[test]
    fn test_node_state_latency_tracking() {
        let mut node = NodeState::new(1, "localhost:9001".to_string());

        // Initial state has no latency
        assert_eq!(node.latency_ms, None);

        // Mark healthy with latency
        node.mark_healthy(50);
        assert_eq!(node.latency_ms, Some(50));

        // Mark degraded with higher latency
        node.mark_degraded(500);
        assert_eq!(node.latency_ms, Some(500));

        // Mark unreachable clears latency
        node.mark_unreachable();
        assert_eq!(node.latency_ms, None);
    }

    #[test]
    fn test_node_state_last_seen_tracking() {
        let mut node = NodeState::new(1, "localhost:9001".to_string());

        // Initial state has no last_seen
        assert!(node.last_seen.is_none());

        // Mark healthy sets last_seen
        node.mark_healthy(10);
        assert!(node.last_seen.is_some());
        let first_seen = node.last_seen.unwrap();

        // Mark degraded updates last_seen
        std::thread::sleep(std::time::Duration::from_millis(1));
        node.mark_degraded(100);
        assert!(node.last_seen.unwrap() >= first_seen);

        // Mark unreachable does NOT update last_seen
        let last_seen = node.last_seen;
        node.mark_unreachable();
        assert_eq!(node.last_seen, last_seen);
    }

    #[test]
    fn test_cluster_state_five_node_quorum() {
        let mut state = ClusterState::new(1);

        // 5-node cluster needs 3 nodes for quorum
        for i in 2..=5 {
            state.nodes.insert(i, {
                let mut n = NodeState::new(i, format!("host{}:9001", i));
                n.mark_healthy(5);
                n
            });
        }

        // All 5 healthy = quorum
        assert!(state.check_quorum());
        assert_eq!(state.total_node_count(), 5);
        assert_eq!(state.available_node_count(), 4); // 4 remote + 1 local

        // 2 unreachable = still quorum (3/5)
        state.nodes.get_mut(&2).unwrap().mark_unreachable();
        state.nodes.get_mut(&3).unwrap().mark_unreachable();
        assert!(state.check_quorum());

        // 3 unreachable = no quorum (2/5)
        state.nodes.get_mut(&4).unwrap().mark_unreachable();
        assert!(!state.check_quorum());
    }

    #[test]
    fn test_partition_status_enum() {
        assert_eq!(PartitionStatus::Connected, PartitionStatus::Connected);
        assert_ne!(PartitionStatus::Connected, PartitionStatus::Minority);
        assert_ne!(PartitionStatus::Minority, PartitionStatus::Unknown);
    }

    #[test]
    fn test_is_possibly_partitioned() {
        let mut state = ClusterState::new(1);

        // No nodes = not partitioned
        assert!(!state.is_possibly_partitioned(Duration::from_secs(30)));

        // Add unreachable node with no last_seen
        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_unreachable();
            n
        });

        // Node has no last_seen, so not "stale"
        assert!(!state.is_possibly_partitioned(Duration::from_secs(30)));

        // Add node that was seen then became unreachable
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_healthy(5); // Sets last_seen
            n.mark_unreachable();
            n
        });

        // With zero threshold, node is immediately stale
        assert!(state.is_possibly_partitioned(Duration::from_secs(0)));
    }

    #[test]
    fn test_cluster_state_with_all_health_states() {
        let mut state = ClusterState::new(1);

        // Add nodes in different states
        state.nodes.insert(2, {
            let mut n = NodeState::new(2, "host2:9001".to_string());
            n.mark_healthy(5);
            n
        });
        state.nodes.insert(3, {
            let mut n = NodeState::new(3, "host3:9001".to_string());
            n.mark_degraded(200);
            n
        });
        state.nodes.insert(4, {
            let mut n = NodeState::new(4, "host4:9001".to_string());
            n.mark_unreachable();
            n
        });
        state
            .nodes
            .insert(5, NodeState::new(5, "host5:9001".to_string())); // Unknown

        assert_eq!(state.healthy_nodes().len(), 1); // Only node 2
        assert_eq!(state.available_node_count(), 2); // Nodes 2 and 3
        assert_eq!(state.unreachable_nodes().len(), 1); // Node 4
        assert_eq!(state.total_node_count(), 5); // 4 remote + 1 local
    }

    #[test]
    fn test_detect_partition_single_node() {
        let state = ClusterState::new(1);

        // Single node is always connected
        assert_eq!(state.detect_partition(), PartitionStatus::Connected);
    }

    #[test]
    fn test_consecutive_failures_accumulate() {
        let mut node = NodeState::new(1, "localhost:9001".to_string());

        for i in 1..=10 {
            node.mark_unreachable();
            assert_eq!(node.consecutive_failures, i);
        }

        // Mark healthy resets counter
        node.mark_healthy(5);
        assert_eq!(node.consecutive_failures, 0);

        // Mark degraded also resets counter
        node.mark_unreachable();
        node.mark_unreachable();
        assert_eq!(node.consecutive_failures, 2);
        node.mark_degraded(100);
        assert_eq!(node.consecutive_failures, 0);
    }
}
