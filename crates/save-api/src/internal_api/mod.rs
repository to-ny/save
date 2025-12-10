//! Internal gRPC API for cluster management.

mod server;
mod service;

pub use server::run_server;
pub use service::ClusterAdminService;
