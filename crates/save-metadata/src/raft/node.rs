//! Raft node management.

use super::network::Network;
use super::storage::Storage;
use super::types::{NodeId, Raft};
use crate::error::Result;
use openraft::storage::Adaptor;
use openraft::{BasicNode, Config};
use save_common::config::ClusterConfig;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

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
        let peers = parse_peers(&cluster_config.peers)?;

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

    /// Initializes the cluster with the given members.
    /// Should only be called on the bootstrap node.
    pub async fn initialize(&self, members: Vec<(NodeId, String)>) -> Result<()> {
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
}

/// Parses peer strings in format "node_id:host:port" to (NodeId, addr) tuples.
fn parse_peers(peers: &[String]) -> Result<Vec<(NodeId, String)>> {
    peers
        .iter()
        .map(|peer| {
            let parts: Vec<&str> = peer.split(':').collect();
            if parts.len() != 3 {
                return Err(crate::error::MetadataError::Raft(format!(
                    "invalid peer format: {}",
                    peer
                )));
            }
            let node_id: NodeId = parts[0].parse().map_err(|_| {
                crate::error::MetadataError::Raft(format!("invalid node_id in peer: {}", peer))
            })?;
            let addr = format!("http://{}:{}", parts[1], parts[2]);
            Ok((node_id, addr))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_peers() {
        let peers = vec![
            "1:192.168.1.10:9001".to_string(),
            "2:192.168.1.11:9001".to_string(),
        ];
        let result = parse_peers(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (1, "http://192.168.1.10:9001".to_string()));
        assert_eq!(result[1], (2, "http://192.168.1.11:9001".to_string()));
    }

    #[test]
    fn test_parse_peers_invalid_format() {
        let peers = vec!["invalid".to_string()];
        assert!(parse_peers(&peers).is_err());
    }

    #[test]
    fn test_parse_peers_invalid_node_id() {
        let peers = vec!["abc:192.168.1.10:9001".to_string()];
        assert!(parse_peers(&peers).is_err());
    }
}
