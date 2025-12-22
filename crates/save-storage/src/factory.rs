//! Storage backend factory for creating backends based on cluster configuration.

use crate::ObjectStorage;
use crate::ReplicationStorage;
use crate::backend::StorageBackend;
use crate::error::Result;
use crate::local_backend::LocalBackend;
use crate::replicated_backend::ReplicatedBackend;
use crate::replication::{QuorumConfig, ReplicationCoordinator};
use save_common::RetryConfig;
use save_common::config::ClusterConfig;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Result of creating a storage backend, includes optional coordinator for multi-node setup.
pub struct StorageSetup {
    pub backend: Arc<dyn StorageBackend>,
    pub coordinator: Option<Arc<ReplicationCoordinator>>,
    /// Storage abstraction for replication service (only set when replication is enabled).
    pub replication_storage: Option<Arc<dyn ReplicationStorage>>,
}

/// Creates a storage backend based on cluster configuration.
///
/// Returns `LocalBackend` for single-node (replication_factor=1) or
/// `ReplicatedBackend` for multi-node clusters.
pub async fn create_storage_backend<P: AsRef<Path>>(
    data_path: P,
    fsync_mode: &str,
    cluster_config: &ClusterConfig,
) -> Result<StorageSetup> {
    let replication_factor = cluster_config.replication.replication_factor;

    if replication_factor <= 1 {
        info!("Creating LocalBackend (replication disabled)");
        let backend = LocalBackend::new_with_fsync_mode(data_path, fsync_mode).await?;
        return Ok(StorageSetup {
            backend: Arc::new(backend),
            coordinator: None,
            replication_storage: None,
        });
    }

    info!(
        replication_factor = replication_factor,
        "Creating ReplicatedBackend"
    );

    let storage = Arc::new(ObjectStorage::new_with_fsync_mode(data_path, fsync_mode).await?);
    let quorum_config = QuorumConfig::with_replication_factor(replication_factor);

    let connect_timeout = Duration::from_secs(cluster_config.connect_timeout_secs);
    let rpc_timeout = Duration::from_secs(cluster_config.rpc_timeout_secs);

    let retry_config: RetryConfig = (&cluster_config.replication.retry).into();
    let coordinator = Arc::new(
        ReplicationCoordinator::with_timeouts(
            cluster_config.node_id,
            quorum_config.clone(),
            connect_timeout,
            rpc_timeout,
        )
        .with_retry_config(retry_config),
    );

    let backend = ReplicatedBackend::with_shared_storage(
        Arc::clone(&storage),
        Arc::clone(&coordinator),
        quorum_config,
    );

    Ok(StorageSetup {
        backend: Arc::new(backend),
        coordinator: Some(coordinator),
        replication_storage: Some(storage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_local_backend_by_default() {
        let temp_dir = TempDir::new().unwrap();
        let config = ClusterConfig::default();

        let setup = create_storage_backend(temp_dir.path(), "data", &config)
            .await
            .unwrap();

        assert!(setup.coordinator.is_none());
    }

    #[tokio::test]
    async fn test_create_replicated_backend_with_factor_3() {
        let temp_dir = TempDir::new().unwrap();
        let mut config = ClusterConfig::default();
        config.replication.replication_factor = 3;

        let setup = create_storage_backend(temp_dir.path(), "data", &config)
            .await
            .unwrap();

        assert!(setup.coordinator.is_some());
    }
}
