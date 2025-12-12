//! Raft network layer implementation using gRPC.

use super::rpc::RaftRpcClient;
use super::types::{NodeId, NodeTypeConfig, SaveNode};
use openraft::error::{NetworkError, RPCError, RaftError};
use openraft::network::{RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Raft network factory that creates connections to peers.
pub struct Network {
    #[allow(dead_code)]
    node_id: NodeId,
    peers: Arc<std::sync::RwLock<HashMap<NodeId, String>>>,
    clients: Arc<RwLock<HashMap<NodeId, RaftRpcClient>>>,
    connect_timeout: Duration,
    rpc_timeout: Duration,
}

impl Network {
    /// Create a new network with default timeouts.
    ///
    /// Peers are specified as (node_id, raft_addr, http_addr) tuples.
    /// Only the raft_addr is used for Raft RPC connections.
    pub fn with_peers(node_id: NodeId, peers: Vec<(NodeId, String, String)>) -> Self {
        Self::with_peers_and_timeouts(
            node_id,
            peers,
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
    }

    /// Create a new network with custom timeouts.
    ///
    /// Peers are specified as (node_id, raft_addr, http_addr) tuples.
    /// Only the raft_addr is used for Raft RPC connections.
    pub fn with_peers_and_timeouts(
        node_id: NodeId,
        peers: Vec<(NodeId, String, String)>,
        connect_timeout: Duration,
        rpc_timeout: Duration,
    ) -> Self {
        // Extract only the Raft addresses for the peer map (used for Raft RPC)
        let peer_map: HashMap<NodeId, String> = peers
            .into_iter()
            .map(|(id, raft_addr, _http_addr)| (id, raft_addr))
            .collect();
        Self {
            node_id,
            peers: Arc::new(std::sync::RwLock::new(peer_map)),
            clients: Arc::new(RwLock::new(HashMap::new())),
            connect_timeout,
            rpc_timeout,
        }
    }

    /// Adds a peer to the network. Used for cluster membership changes.
    #[allow(dead_code)]
    pub fn add_peer(&self, node_id: NodeId, raft_addr: String) {
        let mut peers = self.peers.write().unwrap();
        peers.insert(node_id, raft_addr);
    }

    /// Removes a peer from the network. Used for cluster membership changes.
    #[allow(dead_code)]
    pub fn remove_peer(&self, node_id: NodeId) {
        let mut peers = self.peers.write().unwrap();
        peers.remove(&node_id);
    }
}

impl RaftNetworkFactory<NodeTypeConfig> for Network {
    type Network = NetworkConnection;

    async fn new_client(&mut self, target: NodeId, node: &SaveNode) -> Self::Network {
        let addr = {
            let peers = self.peers.read().unwrap();
            peers.get(&target).cloned()
        };

        // Use peer map address if available, otherwise use node.raft_addr from membership.
        // node.raft_addr may already have "http://" prefix from initialization.
        let endpoint = addr.unwrap_or_else(|| {
            if node.raft_addr.starts_with("http://") || node.raft_addr.starts_with("https://") {
                node.raft_addr.clone()
            } else {
                format!("http://{}", node.raft_addr)
            }
        });

        // Get or create a cached client for this target
        let client = {
            let mut clients = self.clients.write().await;
            clients
                .entry(target)
                .or_insert_with(|| {
                    RaftRpcClient::with_timeouts(
                        endpoint.clone(),
                        self.connect_timeout,
                        self.rpc_timeout,
                    )
                })
                .clone()
        };

        NetworkConnection::new(client)
    }
}

/// Network connection to a specific peer.
///
/// Wraps a cached `RaftRpcClient` that handles connection management
/// and automatic reconnection on transport failures.
pub struct NetworkConnection {
    client: RaftRpcClient,
}

impl NetworkConnection {
    /// Create a new connection wrapping an existing client.
    pub fn new(client: RaftRpcClient) -> Self {
        Self { client }
    }
}

impl RaftNetwork<NodeTypeConfig> for NetworkConnection {
    async fn append_entries(
        &mut self,
        req: AppendEntriesRequest<NodeTypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, SaveNode, RaftError<NodeId>>> {
        self.client
            .append_entries(req)
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.message()))))
    }

    async fn install_snapshot(
        &mut self,
        req: InstallSnapshotRequest<NodeTypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, SaveNode, RaftError<NodeId, openraft::error::InstallSnapshotError>>,
    > {
        self.client
            .install_snapshot(req)
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.message()))))
    }

    async fn vote(
        &mut self,
        req: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, SaveNode, RaftError<NodeId>>> {
        self.client
            .vote(req)
            .await
            .map_err(|e| RPCError::Network(NetworkError::new(&std::io::Error::other(e.message()))))
    }
}
