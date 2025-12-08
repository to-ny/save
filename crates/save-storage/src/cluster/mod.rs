//! Cluster coordination for storage replication.

mod manager;
mod state;

pub use manager::{ClusterEvent, ClusterManager, ClusterManagerConfig, ClusterTopology};
pub use state::{ClusterState, NodeHealth, NodeState, PartitionStatus};
