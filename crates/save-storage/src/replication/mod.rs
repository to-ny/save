//! Replication infrastructure for distributed object storage.
//!
//! This module provides:
//! - `ReplicationService`: gRPC server handling incoming replication requests
//! - `ReplicationClient`: Client for making replication requests to other nodes
//! - `ReplicationCoordinator`: Orchestrates quorum writes across nodes
//! - `HealthChecker`: Health monitoring for replica nodes

mod client;
mod coordinator;
mod health;
mod server;
mod service;

pub use client::ReplicationClient;
pub use coordinator::{QuorumConfig, ReplicationCoordinator, ReplicationResult};
pub use health::{HealthCheckResult, HealthChecker, HealthStatus};
pub use server::run_server;
pub use service::ReplicationService;
