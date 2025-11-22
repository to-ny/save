use super::commands::Command;
use openraft::BasicNode;
use std::io::Cursor;

pub type NodeId = u64;

/// OpenRaft type configuration for save-metadata.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct NodeTypeConfig;

impl openraft::RaftTypeConfig for NodeTypeConfig {
    type D = Command;
    type R = ();
    type NodeId = NodeId;
    type Node = BasicNode;
    type Entry = openraft::Entry<Self>;

    type SnapshotData = Cursor<Vec<u8>>;
    type AsyncRuntime = openraft::TokioRuntime;
    type Responder = openraft::impls::OneshotResponder<Self>;
}

pub type Raft = openraft::Raft<NodeTypeConfig>;
pub type LogId = openraft::LogId<NodeId>;
pub type Vote = openraft::Vote<NodeId>;
pub type Entry = openraft::Entry<NodeTypeConfig>;
