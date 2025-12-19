//! Raft node management.

use super::network::Network;
use super::storage::Storage;
use super::types::{NodeId, Raft, SaveNode};
use crate::error::{MetadataError, Result};
use openraft::error::{ClientWriteError, RaftError};
use openraft::storage::Adaptor;
use openraft::{ChangeMembers, Config, ServerState};
use save_common::cluster::parse_peer;
use save_common::config::ClusterConfig;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

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
    /// IDs of voting members in the cluster.
    pub voters: Vec<NodeId>,
    /// IDs of learner (non-voting) members.
    pub learners: Vec<NodeId>,
    /// Total number of nodes (voters + learners).
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
    ///
    /// Peers are specified as (node_id, raft_addr, http_addr, replication_addr) tuples.
    pub async fn new(
        node_id: NodeId,
        db: Arc<rocksdb::DB>,
        peers: Vec<(NodeId, String, String, String)>,
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
        let peers = parse_peers_to_tuples(&cluster_config.seed_nodes)?;

        let config = Config {
            heartbeat_interval: cluster_config.heartbeat_interval_ms,
            election_timeout_min: cluster_config.election_timeout_min_ms,
            election_timeout_max: cluster_config.election_timeout_max_ms,
            max_in_snapshot_log_to_keep: 1000,
            ..Config::default()
        };

        Self::with_config(node_id, db, peers, config).await
    }

    /// Creates a new Raft node with custom configuration.
    ///
    /// Peers are specified as (node_id, raft_addr, http_addr, replication_addr) tuples.
    pub async fn with_config(
        node_id: NodeId,
        db: Arc<rocksdb::DB>,
        peers: Vec<(NodeId, String, String, String)>,
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
    ///
    /// Members are specified as (node_id, raft_addr, http_addr, replication_addr) tuples.
    pub async fn initialize(&self, members: Vec<(NodeId, String, String, String)>) -> Result<()> {
        if self.is_initialized() {
            return Err(crate::error::MetadataError::AlreadyInitialized);
        }

        let mut nodes = BTreeMap::new();
        for (id, raft_addr, http_addr, replication_addr) in members {
            nodes.insert(id, SaveNode::new(raft_addr, http_addr, replication_addr));
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

    /// Returns the Raft gRPC address for a given node ID from the membership config.
    pub fn get_node_raft_addr(&self, node_id: NodeId) -> Option<String> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics
            .membership_config
            .membership()
            .get_node(&node_id)
            .map(|node| node.raft_addr.clone())
    }

    /// Returns the HTTP API address for a given node ID from the membership config.
    pub fn get_node_http_addr(&self, node_id: NodeId) -> Option<String> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics
            .membership_config
            .membership()
            .get_node(&node_id)
            .map(|node| node.http_addr.clone())
    }

    /// Returns the replication gRPC address for a given node ID from the membership config.
    pub fn get_node_replication_addr(&self, node_id: NodeId) -> Option<String> {
        let metrics = self.raft.metrics().borrow().clone();
        metrics
            .membership_config
            .membership()
            .get_node(&node_id)
            .map(|node| node.replication_addr.clone())
    }

    /// Returns all cluster members with their replication addresses.
    /// Used for syncing with replication coordinator.
    /// Returns (node_id, replication_addr) pairs excluding self.
    pub fn get_cluster_replication_nodes(&self) -> Vec<(NodeId, String)> {
        let metrics = self.raft.metrics().borrow().clone();
        let membership = metrics.membership_config.membership();

        let mut nodes = Vec::new();
        for node_id in membership.voter_ids().chain(membership.learner_ids()) {
            if node_id != self.node_id
                && let Some(node) = membership.get_node(&node_id)
            {
                nodes.push((node_id, node.replication_addr.clone()));
            }
        }
        nodes
    }

    /// Returns the current leader's Raft gRPC address, if known.
    pub async fn leader_raft_addr(&self) -> Option<String> {
        let leader_id = self.current_leader().await?;
        self.get_node_raft_addr(leader_id)
    }

    /// Returns the current leader's HTTP API address, if known.
    ///
    /// This is used for forwarding client write requests to the leader.
    pub async fn leader_http_addr(&self) -> Option<String> {
        let leader_id = self.current_leader().await?;
        self.get_node_http_addr(leader_id)
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
        let voters: Vec<NodeId> = metrics.membership_config.membership().voter_ids().collect();
        let learners: Vec<NodeId> = metrics
            .membership_config
            .membership()
            .learner_ids()
            .collect();
        let initialized = !voters.is_empty();

        ClusterStatus {
            node_id: self.node_id,
            state: metrics.state.into(),
            current_term: metrics.current_term,
            current_leader: metrics.current_leader,
            last_applied_index: metrics.last_applied.map(|id| id.index),
            last_log_index: metrics.last_log_index,
            member_count: voters.len() + learners.len(),
            voters,
            learners,
            initialized,
        }
    }

    /// Adds a node as a non-voting learner.
    /// The learner will receive log replication but cannot vote.
    /// Must be called on the leader node.
    pub async fn add_learner(
        &self,
        node_id: NodeId,
        raft_addr: String,
        http_addr: String,
        replication_addr: String,
    ) -> Result<()> {
        let node = SaveNode::new(raft_addr, http_addr, replication_addr);
        self.raft
            .add_learner(node_id, node, true)
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
        Ok(())
    }

    /// Promotes learners to voting members.
    /// The nodes must already be learners in the cluster.
    /// Must be called on the leader node.
    pub async fn promote_voters(&self, node_ids: Vec<NodeId>) -> Result<()> {
        let members: BTreeSet<NodeId> = node_ids.into_iter().collect();
        self.raft
            .change_membership(ChangeMembers::AddVoterIds(members), false)
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
        Ok(())
    }

    /// Removes voters from the cluster (demotes to learner).
    /// Must be called on the leader node.
    pub async fn remove_voters(&self, node_ids: Vec<NodeId>) -> Result<()> {
        let members: BTreeSet<NodeId> = node_ids.into_iter().collect();
        self.raft
            .change_membership(ChangeMembers::RemoveVoters(members), false)
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
        Ok(())
    }

    /// Removes a node entirely from the cluster (voter and learner).
    /// Must be called on the leader node.
    pub async fn remove_node(&self, node_id: NodeId) -> Result<()> {
        let members: BTreeSet<NodeId> = [node_id].into_iter().collect();
        self.raft
            .change_membership(ChangeMembers::RemoveNodes(members), false)
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
        Ok(())
    }

    /// Returns the list of current learner node IDs.
    pub fn learners(&self) -> Vec<NodeId> {
        let binding = self.raft.metrics();
        let metrics = binding.borrow();
        metrics
            .membership_config
            .membership()
            .learner_ids()
            .collect()
    }

    /// Triggers an election on this node.
    ///
    /// This causes the node to immediately start a leader election,
    /// regardless of the election timeout. Useful for expediting
    /// leadership transfer during graceful shutdown.
    pub async fn trigger_elect(&self) -> Result<()> {
        info!(node_id = self.node_id, "Triggering election");
        self.raft
            .trigger()
            .elect()
            .await
            .map_err(|e| MetadataError::Raft(e.to_string()))?;
        Ok(())
    }

    /// Submits a command through Raft consensus.
    ///
    /// If this node is not the leader, it will wait for a leader to be elected
    /// and retry the operation. This handles leadership changes during operation.
    pub async fn write(&self, command: super::commands::Command) -> Result<()> {
        self.write_with_retry(command, 5, Duration::from_millis(200))
            .await
    }

    /// Internal write with retry logic for leadership changes.
    async fn write_with_retry(
        &self,
        command: super::commands::Command,
        max_retries: u32,
        retry_delay: Duration,
    ) -> Result<()> {
        let mut attempt = 0;
        let mut last_error = None;

        while attempt <= max_retries {
            match self.raft.client_write(command.clone()).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    // Check if this is a ForwardToLeader error (wrapped in RaftError)
                    let is_forward_to_leader = match &e {
                        RaftError::APIError(ClientWriteError::ForwardToLeader(forward)) => {
                            Some(forward.leader_id)
                        }
                        _ => None,
                    };

                    if let Some(leader_id) = is_forward_to_leader {
                        debug!(
                            attempt = attempt,
                            leader_id = ?leader_id,
                            "Not leader, waiting for leader election"
                        );

                        // If we know the leader is us or no leader is known, wait for election
                        if leader_id == Some(self.node_id) || leader_id.is_none() {
                            // Wait for a leader to be elected
                            let wait_result = self.wait_for_leader(Duration::from_secs(5)).await;
                            if wait_result.is_err() {
                                last_error = Some(MetadataError::NotLeader {
                                    leader_id: None,
                                    leader_addr: None,
                                });
                                attempt += 1;
                                tokio::time::sleep(retry_delay).await;
                                continue;
                            }
                        } else {
                            // There's a different leader - this node shouldn't be handling writes
                            // Return the HTTP address for client forwarding
                            let leader_addr = leader_id.and_then(|id| self.get_node_http_addr(id));
                            return Err(MetadataError::NotLeader {
                                leader_id,
                                leader_addr,
                            });
                        }

                        attempt += 1;
                        tokio::time::sleep(retry_delay).await;
                        continue;
                    }

                    // For other errors, fail immediately
                    return Err(MetadataError::Raft(e.to_string()));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| MetadataError::Raft("Max retries exceeded".to_string())))
    }

    pub async fn put_object_metadata(&self, metadata: crate::ObjectMetadata) -> Result<()> {
        self.write(super::commands::Command::put_object_metadata(metadata))
            .await
    }

    pub async fn delete_object_metadata(&self, bucket: String, key: String) -> Result<()> {
        self.write(super::commands::Command::delete_object_metadata(
            bucket, key,
        ))
        .await
    }

    pub async fn create_bucket(&self, name: &str) -> Result<()> {
        let bucket = save_common::Bucket::new(name.to_string());
        self.write(super::commands::Command::create_bucket(bucket))
            .await
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<()> {
        self.write(super::commands::Command::delete_bucket(name.to_string()))
            .await
    }

    /// Confirms leadership and ensures state machine is up-to-date for linearizable reads.
    pub async fn ensure_linearizable(&self) -> Result<()> {
        self.raft
            .ensure_linearizable()
            .await
            .map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
        Ok(())
    }

    pub async fn acquire_lock(
        &self,
        bucket: &str,
        key: &str,
        lock_type: super::commands::LockType,
        holder: super::commands::LockHolder,
    ) -> Result<()> {
        self.write(super::commands::Command::AcquireLock {
            bucket: bucket.to_string(),
            key: key.to_string(),
            lock_type,
            holder,
        })
        .await
    }

    pub async fn release_lock(
        &self,
        bucket: &str,
        key: &str,
        holder: super::commands::LockHolder,
    ) -> Result<()> {
        self.write(super::commands::Command::ReleaseLock {
            bucket: bucket.to_string(),
            key: key.to_string(),
            holder,
        })
        .await
    }
}

/// Parses peer strings using the shared utility and converts to (NodeId, raft_addr, http_addr, replication_addr) tuples.
fn parse_peers_to_tuples(peers: &[String]) -> Result<Vec<(NodeId, String, String, String)>> {
    peers
        .iter()
        .map(|peer| {
            let info =
                parse_peer(peer).map_err(|e| crate::error::MetadataError::Raft(e.to_string()))?;
            Ok((
                info.node_id,
                info.raft_addr(),
                info.http_addr(),
                info.replication_addr(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_peers_to_tuples_full_format() {
        let peers = vec![
            "1:192.168.1.10:9001:9000:9002".to_string(),
            "2:192.168.1.11:9001:9000:9002".to_string(),
        ];
        let result = parse_peers_to_tuples(&peers).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0],
            (
                1,
                "http://192.168.1.10:9001".to_string(),
                "http://192.168.1.10:9000".to_string(),
                "http://192.168.1.10:9002".to_string()
            )
        );
        assert_eq!(
            result[1],
            (
                2,
                "http://192.168.1.11:9001".to_string(),
                "http://192.168.1.11:9000".to_string(),
                "http://192.168.1.11:9002".to_string()
            )
        );
    }

    #[test]
    fn test_parse_peers_to_tuples_legacy_format() {
        let peers = vec![
            "1:192.168.1.10:9001".to_string(),
            "2:192.168.1.11:9001".to_string(),
        ];
        let result = parse_peers_to_tuples(&peers).unwrap();
        assert_eq!(result.len(), 2);
        // Legacy format: HTTP port defaults to 9000, replication to 9002
        assert_eq!(
            result[0],
            (
                1,
                "http://192.168.1.10:9001".to_string(),
                "http://192.168.1.10:9000".to_string(),
                "http://192.168.1.10:9002".to_string()
            )
        );
        assert_eq!(
            result[1],
            (
                2,
                "http://192.168.1.11:9001".to_string(),
                "http://192.168.1.11:9000".to_string(),
                "http://192.168.1.11:9002".to_string()
            )
        );
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
