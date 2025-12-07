//! Raft consensus integration for distributed metadata replication.
//!
//! This module provides the Raft consensus layer for distributed metadata:
//! - `storage` - RocksDB-backed Raft log and state storage
//! - `log_store` - Log entry persistence operations
//! - `state_machine` - Command application to the state machine
//! - `snapshot` - Checkpoint-based snapshots for state transfer
//! - `node` - Raft node management and cluster operations
//! - `rpc` - Inter-node RPC communication

mod commands;
#[allow(clippy::result_large_err)]
mod log_store;
mod network;
mod node;
mod rpc;
mod server;
mod snapshot;
#[allow(clippy::result_large_err)]
mod state_machine;
#[allow(clippy::result_large_err)]
mod storage;
mod types;

pub use commands::{Command, LockHolder, LockType};
pub use node::{ClusterStatus, RaftNode, RaftState};
pub use rpc::{RaftRpcClient, RaftRpcServer};
pub use server::run_server;
pub use storage::Storage;
pub use types::{NodeId, NodeTypeConfig};
