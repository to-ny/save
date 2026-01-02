//! Replication infrastructure for distributed object storage.

mod client;
mod coordinator;
mod health;
mod server;
mod service;

pub use client::ReplicationClient;
pub use coordinator::{QuorumConfig, ReplicationCoordinator, ReplicationResult};
pub use health::{HealthCheckResult, HealthChecker, HealthStatus};
pub use server::{run_server, run_server_with_tls};
pub use service::{ReplicationService, StaleCleanupConfig};
