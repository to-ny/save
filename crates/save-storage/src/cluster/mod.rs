//! Cluster coordination for storage replication.
//!
//! Provides node discovery, health monitoring, and cluster state tracking.

mod manager;
mod state;

pub use manager::{ClusterEvent, ClusterManager, ClusterManagerConfig, ClusterTopology};
pub use state::{ClusterState, NodeHealth, NodeState, PartitionStatus};
