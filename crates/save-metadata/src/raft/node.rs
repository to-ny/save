//! Raft node management.

use super::network::Network;
use super::storage::Storage;
use super::types::{NodeId, Raft};
use crate::error::Result;
use openraft::storage::Adaptor;
use openraft::{BasicNode, Config, ServerState};
use save_common::cluster::parse_peer;
use save_common::config::ClusterConfig;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// Raft node state for monitoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RaftState {
    /// This node is the cluster leader.
    Leader,
    /// This node is a follower.
    Follower,
    /// This node is a candidate for leadership.
    Candidate,
    /// This node is a learner (non-voting member).
    Learner,
    /// This node is shutting down.
    Shutdown,
}

impl From<ServerState> for RaftState {
    fn from(state: ServerState) -> Self {
        match state {
            ServerState::Leader => RaftState::Leader,
            ServerState::Follower => RaftState::Follower,
            ServerState::Candidate => RaftState::Candidate,
            ServerState::Learner => RaftState::Learner,
            ServerState::Shutdown => RaftState::Shutdown,
        }
    }
}

/// Cluster status information exposed for monitoring.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterStatus {
    /// This node's ID.
    pub node_id: NodeId,
    /// Current Raft state.
    pub state: RaftState,
    /// Current term number.
    pub current_term: u64,
    /// Current leader ID, if known.
    pub current_leader: Option<NodeId>,
    /// Last applied log index.
    pub last_applied_index: Option<u64>,
    /// Last log index.
    pub last_log_index: Option<u64>,
    /// IDs of nodes in the cluster.
    pub members: Vec<NodeId>,
    /// Number of nodes in the cluster.
    pub member_count: usize,
    /// Whether the cluster has been initialized.
    pub initialized: bool,
}

/// Raft node managing consensus and replication.
pub struct RaftNode {
    raft: Arc<Raft>,
    node_id: NodeId,
}

impl RaftNode {
    /// Creates a new Raft node with default configuration.
    pub async fn new(
        node_id: NodeId,
        db: Arc<rocksdb::DB>,
        peers: Vec<(NodeId, String)>,
    ) -> Result<Self> {
        let config = Config::default();
        Self::with_config(node_id, db, peers, config).await
    }

    /// Creates a new Raft node from ClusterConfig.
    pub async fn from_cluster_config(
        db: Arc<rocksdb::DB>,
        cluster_config: &ClusterConfig,
    ) -> Result<Self> {
        let node_id = cluster_config.node_id;
        let peers = parse_peers_to_tuples(&cluster_config.peers)?;

        let config = Config {
            heartbeat_interval: 150,
            election_timeout_min: 300,
            election_timeout_max: 600,
            max_in_snapshot_log_to_keep: 1000,
            ..Config::default()
        };

        Self::with_config(node_id, db, peers, config).await
    }

    /// Creates a new Raft node with custom configuration.
    pub async fn with_config(
        node_id: NodeId,
        db: Arc<rocksdb::DB>,
        peers: Vec<(NodeId, String)>,
        config: Config,
    ) -> Result<Self> {
        let storage = Storage::new(Arc::clone(&db));
        let (log_store, state_machine) = Adaptor::new(storage);
        let network = Network::with_peers(node_id, peers);

        let raft = openraft::Raft::new(node_id, config.into(), network, log_store, state_machine)
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;

        Ok(Self {
            raft: Arc::new(raft),
            node_id,
        })
    }

    /// Returns the node ID.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Returns a reference to the Raft instance.
    pub fn raft(&self) -> &Arc<Raft> {
        &self.raft
    }

    /// Returns true if the cluster has been initialized.
    pub fn is_initialized(&self) -> bool {
        let binding = self.raft.metrics();
        let metrics = binding.borrow();
        !metrics
            .membership_config
            .membership()
            .voter_ids()
            .collect::<Vec<_>>()
            .is_empty()
    }

    /// Initializes the cluster with the given members.
    /// Should only be called on the bootstrap node.
    /// Returns an error if already initialized.
    pub async fn initialize(&self, members: Vec<(NodeId, String)>) -> Result<()> {
        if self.is_initialized() {
            return Err(crate::error::MetadataError::Raft(
                "cluster already initialized".to_string(),
            ));
        }

        let mut nodes = BTreeMap::new();
        for (id, addr) in members {
            nodes.insert(id, BasicNode { addr });
        }

        self.raft
            .initialize(nodes)
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;

        Ok(())
    }

    /// Checks if this node is the leader.
    pub async fn is_leader(&self) -> bool {
        self.raft.current_leader().await == Some(self.node_id)
    }

    /// Returns the current leader ID, if known.
    pub async fn current_leader(&self) -> Option<NodeId> {
        self.raft.current_leader().await
    }

    /// Waits until a leader is elected or timeout.
    pub async fn wait_for_leader(&self, timeout: Duration) -> Result<NodeId> {
        let start = std::time::Instant::now();
        loop {
            if let Some(leader) = self.raft.current_leader().await {
                return Ok(leader);
            }
            if start.elapsed() > timeout {
                return Err(crate::error::MetadataError::Raft(
                    "timeout waiting for leader".to_string(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Returns the current cluster status for monitoring.
    pub fn get_status(&self) -> ClusterStatus {
        let metrics = self.raft.metrics().borrow().clone();
        let members: Vec<NodeId> = metrics.membership_config.membership().voter_ids().collect();
        let initialized = !members.is_empty();

        ClusterStatus {
            node_id: self.node_id,
            state: metrics.state.into(),
            current_term: metrics.current_term,
            current_leader: metrics.current_leader,
            last_applied_index: metrics.last_applied.map(|id| id.index),
            last_log_index: metrics.last_log_index,
            member_count: members.len(),
            members,
            initialized,
        }
    }
}

/// Parses peer strings using the shared utility and converts to (NodeId, addr) tuples.
fn parse_peers_to_tuples(peers: &[String]) -> Result<Vec<(NodeId, String)>> {
    peers
        .iter()
        .map(|peer| {
            let info =
                parse_peer(peer).map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
            Ok((info.node_id, info.http_addr()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_peers_to_tuples() {
        let peers = vec![
            "1:192.168.1.10:9001".to_string(),
            "2:192.168.1.11:9001".to_string(),
        ];
        let result = parse_peers_to_tuples(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (1, "http://192.168.1.10:9001".to_string()));
        assert_eq!(result[1], (2, "http://192.168.1.11:9001".to_string()));
    }

    #[test]
    fn test_parse_peers_invalid_format() {
        let peers = vec!["invalid".to_string()];
        assert!(parse_peers_to_tuples(&peers).is_err());
    }

    #[test]
    fn test_parse_peers_invalid_node_id() {
        let peers = vec!["abc:192.168.1.10:9001".to_string()];
        assert!(parse_peers_to_tuples(&peers).is_err());
    }

    #[test]
    fn test_raft_state_serialization() {
        assert_eq!(
            serde_json::to_string(&RaftState::Leader).unwrap(),
            "\"leader\""
        );
        assert_eq!(
            serde_json::to_string(&RaftState::Follower).unwrap(),
            "\"follower\""
        );
        assert_eq!(
            serde_json::to_string(&RaftState::Candidate).unwrap(),
            "\"candidate\""
        );
    }
}
