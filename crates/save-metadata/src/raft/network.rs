use super::types::{NodeId, NodeTypeConfig};
use openraft::error::NetworkError;
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};

/// Raft network layer (stub - requires gRPC implementation).
pub struct Network {
    _node_id: NodeId,
}

impl Network {
    pub fn new(node_id: NodeId) -> Self {
        Self { _node_id: node_id }
    }
}

impl RaftNetworkFactory<NodeTypeConfig> for Network {
    type Network = NetworkConnection;

    async fn new_client(&mut self, _target: NodeId, _node: &openraft::BasicNode) -> Self::Network {
        NetworkConnection
    }
}

/// Network connection to a specific peer (stub implementation).
pub struct NetworkConnection;

impl RaftNetwork<NodeTypeConfig> for NetworkConnection {
    async fn append_entries(
        &mut self,
        _req: openraft::raft::AppendEntriesRequest<NodeTypeConfig>,
        _option: RPCOption,
    ) -> Result<
        openraft::raft::AppendEntriesResponse<NodeId>,
        openraft::error::RPCError<NodeId, openraft::BasicNode, openraft::error::RaftError<NodeId>>,
    > {
        Err(openraft::error::RPCError::Network(NetworkError::new(
            &std::io::Error::other("Network not implemented"),
        )))
    }

    async fn install_snapshot(
        &mut self,
        _req: openraft::raft::InstallSnapshotRequest<NodeTypeConfig>,
        _option: RPCOption,
    ) -> Result<
        openraft::raft::InstallSnapshotResponse<NodeId>,
        openraft::error::RPCError<
            NodeId,
            openraft::BasicNode,
            openraft::error::RaftError<NodeId, openraft::error::InstallSnapshotError>,
        >,
    > {
        Err(openraft::error::RPCError::Network(NetworkError::new(
            &std::io::Error::other(
                "Raft network layer not implemented - requires gRPC replication service",
            ),
        )))
    }

    async fn vote(
        &mut self,
        _req: openraft::raft::VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<
        openraft::raft::VoteResponse<NodeId>,
        openraft::error::RPCError<NodeId, openraft::BasicNode, openraft::error::RaftError<NodeId>>,
    > {
        Err(openraft::error::RPCError::Network(NetworkError::new(
            &std::io::Error::other(
                "Raft network layer not implemented - requires gRPC replication service",
            ),
        )))
    }
}
