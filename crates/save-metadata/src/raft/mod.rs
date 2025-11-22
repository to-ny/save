//! Raft consensus integration for distributed metadata replication.
//!
//! All implementations are stubs. See `tasks/phase2.md` for roadmap.

mod commands;
mod network;
mod node;
mod snapshot;
mod storage;
mod types;

pub use commands::Command;
pub use node::RaftNode;
pub use storage::Storage;
pub use types::{NodeId, NodeTypeConfig};
