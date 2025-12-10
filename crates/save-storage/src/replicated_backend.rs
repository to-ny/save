//! Replicated storage backend using 2PC for quorum writes.

use crate::ObjectStorage;
use crate::backend::{HealthStatus, StorageBackend, TempHandle};
use crate::error::{Result, StorageError};
use crate::replication::{QuorumConfig, ReplicationCoordinator};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Compute SHA256 checksum of a file without loading it entirely into memory.
async fn compute_file_checksum(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(StorageError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024]; // 64KB chunks

    loop {
        let n = file.read(&mut buf).await.map_err(StorageError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// TempHandle for replicated storage. Stores key for replication at commit time.
#[derive(Debug)]
pub struct ReplicatedTempHandle {
    inner: crate::TempObject,
    key: String,
}

impl ReplicatedTempHandle {
    fn new(temp_object: crate::TempObject, key: String) -> Self {
        Self {
            inner: temp_object,
            key,
        }
    }
}

impl TempHandle for ReplicatedTempHandle {
    fn temp_path(&self) -> &Path {
        self.inner.temp_path()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Replicated storage backend using 2PC for quorum writes.
pub struct ReplicatedBackend {
    local: ObjectStorage,
    coordinator: Arc<ReplicationCoordinator>,
    quorum_config: QuorumConfig,
}

impl ReplicatedBackend {
    pub fn new(
        local: ObjectStorage,
        coordinator: Arc<ReplicationCoordinator>,
        quorum_config: QuorumConfig,
    ) -> Self {
        Self {
            local,
            coordinator,
            quorum_config,
        }
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.local.temp_dir()
    }

    pub fn coordinator(&self) -> &ReplicationCoordinator {
        &self.coordinator
    }
}

impl std::fmt::Debug for ReplicatedBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplicatedBackend")
            .field("quorum_config", &self.quorum_config)
            .finish()
    }
}

#[async_trait]
impl StorageBackend for ReplicatedBackend {
    async fn put_object(
        &self,
        key: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<()> {
        // Write to local temp first
        let temp_object = self.local.write_temp_object(key, reader).await?;
        let temp_path = temp_object.temp_path().to_path_buf();

        // Compute checksum from temp file (streaming, no full buffer)
        let checksum = compute_file_checksum(&temp_path).await?;

        // Replicate using streaming and commit
        let result = self
            .coordinator
            .replicate_write_streaming(
                key,
                &temp_path,
                &checksum,
                self.local.commit_object(temp_object),
            )
            .await?;

        if !result.quorum_achieved {
            return Err(StorageError::QuorumNotAchieved {
                achieved: result.success_count,
                required: self.quorum_config.write_quorum,
            });
        }

        Ok(())
    }

    async fn get_object(&self, key: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        // Try local first (preference: local > remote)
        match self.local.get_object(key).await {
            Ok(file) => Ok(Box::new(file) as Box<dyn AsyncRead + Send + Unpin>),
            Err(StorageError::NotFound(_)) => {
                // Fallback to remote replicas (streaming)
                self.coordinator
                    .read_from_replica(key)
                    .await
                    .ok_or_else(|| StorageError::NotFound(key.to_string()))
            }
            Err(e) => Err(e),
        }
    }

    async fn delete_object(&self, key: &str) -> Result<()> {
        let result = self
            .coordinator
            .replicate_delete(key, self.local.delete_object(key))
            .await?;

        tracing::debug!(
            key = %key,
            success_count = result.success_count,
            failure_count = result.failure_count,
            successful_nodes = ?result.successful_nodes,
            failed_nodes = ?result.failed_nodes,
            "Delete replication result"
        );

        if result.success_count == 0 {
            return Err(StorageError::NotFound(key.to_string()));
        }

        Ok(())
    }

    async fn write_temp_object(
        &self,
        key: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<Box<dyn TempHandle>> {
        let temp_object = self.local.write_temp_object(key, reader).await?;
        Ok(
            Box::new(ReplicatedTempHandle::new(temp_object, key.to_string()))
                as Box<dyn TempHandle>,
        )
    }

    async fn commit_object(&self, temp: Box<dyn TempHandle>) -> Result<()> {
        let temp_any = temp.into_any();
        let handle = temp_any.downcast::<ReplicatedTempHandle>().map_err(|_| {
            StorageError::Io(std::io::Error::other(
                "TempHandle must be ReplicatedTempHandle for ReplicatedBackend",
            ))
        })?;

        let temp_path = handle.inner.temp_path().to_path_buf();
        let key = handle.key.clone();

        // Compute checksum from temp file (streaming, no full buffer)
        let checksum = compute_file_checksum(&temp_path).await?;

        // Replicate using streaming and commit local
        let result = self
            .coordinator
            .replicate_write_streaming(
                &key,
                &temp_path,
                &checksum,
                self.local.commit_object(handle.inner),
            )
            .await?;

        if !result.quorum_achieved {
            return Err(StorageError::QuorumNotAchieved {
                achieved: result.success_count,
                required: self.quorum_config.write_quorum,
            });
        }

        Ok(())
    }

    async fn health_check(&self) -> Result<HealthStatus> {
        let temp_dir = self.local.temp_dir();

        match tokio::fs::metadata(&temp_dir).await {
            Ok(metadata) if metadata.is_dir() => {
                let nodes = self.coordinator.connected_nodes().await;
                // Local node counts toward quorum, so we need (write_quorum - 1) remote replicas
                let min_replicas = self.quorum_config.write_quorum.saturating_sub(1);

                if nodes.len() < min_replicas {
                    return Ok(HealthStatus::Degraded {
                        reason: format!(
                            "Only {} replica nodes connected, need {} for quorum",
                            nodes.len(),
                            min_replicas
                        ),
                    });
                }

                Ok(HealthStatus::Healthy)
            }
            Ok(_) => Ok(HealthStatus::Unhealthy {
                reason: "Temp directory path exists but is not a directory".to_string(),
            }),
            Err(e) => Ok(HealthStatus::Unhealthy {
                reason: format!("Cannot access temp directory: {}", e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_replicated_backend_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(1);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        let backend = ReplicatedBackend::new(storage, coordinator, config);
        assert!(backend.temp_dir().exists());
    }

    #[tokio::test]
    async fn test_health_check_single_node() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(1);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        let backend = ReplicatedBackend::new(storage, coordinator, config);
        let health = backend.health_check().await.unwrap();
        assert_eq!(health, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_put_and_get_single_node() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(1);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        let backend = ReplicatedBackend::new(storage, coordinator, config);

        let key = "test/object.txt";
        let data = b"Hello, World!";
        let mut reader = &data[..];

        backend.put_object(key, &mut reader).await.unwrap();

        let mut file = backend.get_object(key).await.unwrap();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn test_write_temp_and_commit() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(1);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        let backend = ReplicatedBackend::new(storage, coordinator, config);

        let key = "test/temp.txt";
        let data = b"Temp data";
        let mut reader = &data[..];

        let temp = backend.write_temp_object(key, &mut reader).await.unwrap();
        assert!(temp.temp_path().exists());

        backend.commit_object(temp).await.unwrap();

        let mut file = backend.get_object(key).await.unwrap();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn test_delete_object() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(1);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        let backend = ReplicatedBackend::new(storage, coordinator, config);

        let key = "test/delete.txt";
        let data = b"Delete me";
        let mut reader = &data[..];

        backend.put_object(key, &mut reader).await.unwrap();
        backend.delete_object(key).await.unwrap();

        let result = backend.get_object(key).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_temp_handle_cleanup_on_drop() {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(1);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        let backend = ReplicatedBackend::new(storage, coordinator, config);

        let key = "test/cleanup.txt";
        let data = b"Cleanup data";
        let mut reader = &data[..];

        let temp_path;
        {
            let temp = backend.write_temp_object(key, &mut reader).await.unwrap();
            temp_path = temp.temp_path().to_path_buf();
            assert!(temp_path.exists());
            // Drop temp without committing
        }

        // Temp file should be cleaned up
        assert!(!temp_path.exists());
    }
}

/// Integration tests using actual gRPC servers for multi-node replication.
#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::replication::{ReplicationService, run_server};
    use std::net::SocketAddr;
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;
    use tokio::sync::broadcast;
    use tokio::time::{Duration, timeout};

    struct TestNode {
        _temp_dir: TempDir,
        addr: SocketAddr,
        shutdown_tx: broadcast::Sender<()>,
    }

    impl TestNode {
        async fn start() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let storage = Arc::new(ObjectStorage::new(temp_dir.path()).await.unwrap());
            let service = Arc::new(ReplicationService::new(storage));

            // Bind to port 0 to let OS assign an available port
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            drop(listener); // Release so tonic can bind

            let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

            let service_clone = service.clone();
            tokio::spawn(async move {
                let _ = run_server(service_clone, addr, Some(shutdown_rx)).await;
            });

            // Wait for server to be ready by attempting connection
            let endpoint = format!("http://{}", addr);
            for _ in 0..50 {
                if let Ok(channel) = tonic::transport::Channel::from_shared(endpoint.clone())
                    .unwrap()
                    .connect_timeout(Duration::from_millis(100))
                    .connect()
                    .await
                {
                    drop(channel);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }

            Self {
                _temp_dir: temp_dir,
                addr,
                shutdown_tx,
            }
        }

        fn endpoint(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    impl Drop for TestNode {
        fn drop(&mut self) {
            let _ = self.shutdown_tx.send(());
        }
    }

    #[tokio::test]
    async fn test_write_with_all_replicas_healthy() {
        let node1 = TestNode::start().await;
        let node2 = TestNode::start().await;

        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(3); // quorum = 2
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        // Add replica nodes
        coordinator.add_node(2, node1.endpoint()).await.unwrap();
        coordinator.add_node(3, node2.endpoint()).await.unwrap();

        assert_eq!(coordinator.connected_nodes().await.len(), 2);

        let backend = ReplicatedBackend::new(local_storage, coordinator, config);

        let key = "test/replicated.txt";
        let data = b"Replicated data";
        let mut reader = &data[..];

        backend.put_object(key, &mut reader).await.unwrap();

        // Verify local read
        let mut file = backend.get_object(key).await.unwrap();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn test_write_with_one_replica_failure_quorum_met() {
        // Start only one replica (node 2 exists, node 3 doesn't)
        let node2 = TestNode::start().await;

        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(3); // quorum = 2
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        // Add one healthy replica
        coordinator.add_node(2, node2.endpoint()).await.unwrap();
        // Node 3 is not added - simulates unreachable node

        let backend = ReplicatedBackend::new(local_storage, coordinator, config);

        let key = "test/partial.txt";
        let data = b"Partial replication";
        let mut reader = &data[..];

        // Should succeed: local (node 1) + node 2 = 2, meets quorum of 2
        backend.put_object(key, &mut reader).await.unwrap();

        // Verify local read
        let mut file = backend.get_object(key).await.unwrap();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn test_write_with_quorum_failure() {
        // No replicas available - only local node
        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(3); // quorum = 2
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        // No replicas added - simulates all replicas unreachable

        let backend = ReplicatedBackend::new(local_storage, coordinator, config);

        let key = "test/fail.txt";
        let data = b"Should fail";
        let mut reader = &data[..];

        // Should fail: only local (node 1) = 1, doesn't meet quorum of 2
        let result = backend.put_object(key, &mut reader).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        match err {
            StorageError::QuorumNotAchieved { achieved, required } => {
                assert_eq!(achieved, 1);
                assert_eq!(required, 2);
            }
            _ => panic!("Expected QuorumNotAchieved error, got: {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_read_after_write_consistency() {
        let node2 = TestNode::start().await;

        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(2); // quorum = 2
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        coordinator.add_node(2, node2.endpoint()).await.unwrap();

        let backend = ReplicatedBackend::new(local_storage, coordinator, config);

        // Write multiple objects
        for i in 0..5 {
            let key = format!("test/obj-{}.txt", i);
            let data = format!("Data for object {}", i);
            let mut reader = data.as_bytes();
            backend.put_object(&key, &mut reader).await.unwrap();
        }

        // Read back all objects
        for i in 0..5 {
            let key = format!("test/obj-{}.txt", i);
            let expected = format!("Data for object {}", i);

            let mut file = backend.get_object(&key).await.unwrap();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await.unwrap();
            assert_eq!(String::from_utf8(buf).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn test_delete_replicates_to_nodes() {
        let node2 = TestNode::start().await;

        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(2);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        coordinator.add_node(2, node2.endpoint()).await.unwrap();

        let backend = ReplicatedBackend::new(local_storage, coordinator.clone(), config);

        let key = "test/to-delete.txt";
        let data = b"Delete me";
        let mut reader = &data[..];

        backend.put_object(key, &mut reader).await.unwrap();

        // Verify exists
        let mut file = backend.get_object(key).await.unwrap();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, data);

        // Delete
        backend.delete_object(key).await.unwrap();

        // Verify deleted from all replicas (get_object fallback to remote should also fail)
        let result = backend.get_object(key).await;
        assert!(
            result.is_err(),
            "Object should be deleted from all replicas"
        );
    }

    #[tokio::test]
    async fn test_health_check_degraded_with_insufficient_replicas() {
        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(3); // quorum = 2, needs 1 replica
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        // No replicas connected

        let backend = ReplicatedBackend::new(local_storage, coordinator, config);
        let health = backend.health_check().await.unwrap();

        match health {
            HealthStatus::Degraded { reason } => {
                assert!(reason.contains("replica nodes connected"));
            }
            _ => panic!("Expected Degraded status, got {:?}", health),
        }
    }

    #[tokio::test]
    async fn test_read_from_remote_when_local_missing() {
        // This test simulates a scenario where data exists on remote but not locally.
        // Write to remote node directly, then try to read from local backend.
        let remote_node = TestNode::start().await;

        // Create local backend that has no data
        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(2);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        // Add remote node
        coordinator
            .add_node(2, remote_node.endpoint())
            .await
            .unwrap();

        let backend = ReplicatedBackend::new(local_storage, coordinator, config);

        // Write directly to remote node via its storage (simulating data that exists remotely)
        let remote_storage = ObjectStorage::new(remote_node._temp_dir.path())
            .await
            .unwrap();
        let key = "test/remote-only.txt";
        let data = b"Remote data only";
        remote_storage.put_object(key, &data[..]).await.unwrap();

        // Try to read from backend - should fall back to remote
        let mut reader = backend
            .get_object(key)
            .await
            .expect("Should read from remote");
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn test_concurrent_writes_to_replicas() {
        let node2 = TestNode::start().await;

        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(2);
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        coordinator.add_node(2, node2.endpoint()).await.unwrap();

        let backend = Arc::new(ReplicatedBackend::new(local_storage, coordinator, config));

        // Launch concurrent writes
        let mut handles = Vec::new();
        for i in 0..10 {
            let backend = backend.clone();
            handles.push(tokio::spawn(async move {
                let key = format!("test/concurrent-{}.txt", i);
                let data = format!("Concurrent data {}", i);
                let mut reader = data.as_bytes();
                backend.put_object(&key, &mut reader).await
            }));
        }

        // Wait for all writes
        for handle in handles {
            let result = timeout(Duration::from_secs(10), handle).await;
            assert!(result.is_ok(), "Write timed out");
            assert!(result.unwrap().unwrap().is_ok(), "Write failed");
        }

        // Verify all objects
        for i in 0..10 {
            let key = format!("test/concurrent-{}.txt", i);
            let expected = format!("Concurrent data {}", i);

            let mut file = backend.get_object(&key).await.unwrap();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await.unwrap();
            assert_eq!(String::from_utf8(buf).unwrap(), expected);
        }
    }

    #[tokio::test]
    async fn test_strong_consistency_reads_during_network_delay() {
        // This test verifies read consistency when there are network delays.
        // We write data to a cluster and verify that reads always return
        // the latest written data, even with simulated delays between operations.

        let node2 = TestNode::start().await;
        let node3 = TestNode::start().await;

        let temp_dir = TempDir::new().unwrap();
        let local_storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let config = QuorumConfig::with_replication_factor(3); // quorum = 2
        let coordinator = Arc::new(ReplicationCoordinator::new(1, config.clone()));

        coordinator.add_node(2, node2.endpoint()).await.unwrap();
        coordinator.add_node(3, node3.endpoint()).await.unwrap();

        let backend = Arc::new(ReplicatedBackend::new(local_storage, coordinator, config));

        // Perform a series of writes with simulated network delays
        for i in 0..10 {
            let key = "test/delayed.txt";
            let data = format!("Version {}", i);
            let mut reader = data.as_bytes();

            // Write new version
            backend.put_object(key, &mut reader).await.unwrap();

            // Simulate network delay between write and read
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Strong consistency read should always return the latest version
            let mut file = backend.get_object(key).await.unwrap();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await.unwrap();

            let read_value = String::from_utf8(buf).unwrap();
            assert_eq!(
                read_value, data,
                "Read should return latest version {} but got {}",
                data, read_value
            );
        }

        // Test concurrent read during write (interleaved operations)
        let backend_clone = backend.clone();
        let write_handle = tokio::spawn(async move {
            for i in 0..5 {
                let key = format!("test/concurrent-{}.txt", i);
                let data = format!("Concurrent data {}", i);
                let mut reader = data.as_bytes();

                // Simulate variable network delay before write
                tokio::time::sleep(Duration::from_millis(i as u64 * 10)).await;
                backend_clone.put_object(&key, &mut reader).await.unwrap();
            }
        });

        // Read operations with delays
        let backend_clone2 = backend.clone();
        let read_handle = tokio::spawn(async move {
            // Wait for first writes to complete
            tokio::time::sleep(Duration::from_millis(30)).await;

            for i in 0..5 {
                let key = format!("test/concurrent-{}.txt", i);
                tokio::time::sleep(Duration::from_millis(20)).await;

                // Object may or may not exist depending on timing
                let result = backend_clone2.get_object(&key).await;
                if let Ok(mut file) = result {
                    let mut buf = Vec::new();
                    file.read_to_end(&mut buf).await.unwrap();
                    let value = String::from_utf8(buf).unwrap();
                    // If we can read it, it should be consistent
                    assert!(
                        value.starts_with("Concurrent data"),
                        "Read value should be valid: {}",
                        value
                    );
                }
            }
        });

        write_handle.await.unwrap();
        read_handle.await.unwrap();

        // Final verification: all concurrent writes should be readable now
        for i in 0..5 {
            let key = format!("test/concurrent-{}.txt", i);
            let mut file = backend.get_object(&key).await.unwrap();
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await.unwrap();

            let expected = format!("Concurrent data {}", i);
            assert_eq!(String::from_utf8(buf).unwrap(), expected);
        }
    }
}
