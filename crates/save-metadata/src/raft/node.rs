use super::network::Network;
use super::storage::Storage;
use super::types::{NodeId, Raft};
use crate::error::Result;
use openraft::Config;
use openraft::storage::Adaptor;
use std::sync::Arc;

/// Raft node managing consensus and replication.
pub struct RaftNode {
    raft: Arc<Raft>,
    node_id: NodeId,
}

impl RaftNode {
    /// Creates a new Raft node.
    pub async fn new(
        node_id: NodeId,
        db: Arc<rocksdb::DB>,
        _peers: Vec<(NodeId, String)>,
    ) -> Result<Self> {
        let config = Config::default();
        let storage = Storage::new(Arc::clone(&db));
        let (log_store, state_machine) = Adaptor::new(storage);
        let network = Network::new(node_id);

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
}
