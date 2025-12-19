use super::commands::Command;
use serde::{Deserialize, Serialize};
use std::io::Cursor;

pub type NodeId = u64;

/// Custom node type for Save that stores Raft, HTTP, and replication addresses.
///
/// This enables proper request forwarding and data replication:
/// - Raft address: Raft consensus gRPC (e.g., "http://host:9001")
/// - HTTP address: Client API (e.g., "http://host:9000")
/// - Replication address: Object data replication gRPC (e.g., "http://host:9002")
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SaveNode {
    /// Raft gRPC address (e.g., "http://host:9001")
    pub raft_addr: String,
    /// HTTP API address (e.g., "http://host:9000")
    pub http_addr: String,
    /// Replication gRPC address (e.g., "http://host:9002")
    pub replication_addr: String,
}

impl SaveNode {
    /// Creates a new SaveNode with the given addresses.
    pub fn new(raft_addr: String, http_addr: String, replication_addr: String) -> Self {
        Self {
            raft_addr,
            http_addr,
            replication_addr,
        }
    }
}

impl std::fmt::Display for SaveNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "raft={}, http={}, replication={}",
            self.raft_addr, self.http_addr, self.replication_addr
        )
    }
}

/// OpenRaft type configuration for save-metadata.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct NodeTypeConfig;

impl openraft::RaftTypeConfig for NodeTypeConfig {
    type D = Command;
    type R = ();
    type NodeId = NodeId;
    type Node = SaveNode;
    type Entry = openraft::Entry<Self>;

    type SnapshotData = Cursor<Vec<u8>>;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder = openraft::impls::OneshotResponder<Self>;
}

pub type Raft = openraft::Raft<NodeTypeConfig>;
pub type LogId = openraft::LogId<NodeId>;
pub type Vote = openraft::Vote<NodeId>;
pub type Entry = openraft::Entry<NodeTypeConfig>;
