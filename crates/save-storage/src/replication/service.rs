//! Replication service implementation handling incoming gRPC requests.

use crate::ReplicationStorage;
use save_proto::replication as proto;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Pending prepare operations awaiting commit/abort.
struct PendingPrepare {
    temp_object: crate::TempObject,
    #[allow(dead_code)]
    checksum: String,
    created_at: Instant,
}

/// Replication service handling incoming object replication requests.
pub struct ReplicationService {
    storage: Arc<dyn ReplicationStorage>,
    start_time: Instant,
    pending_prepares: Arc<RwLock<HashMap<String, PendingPrepare>>>,
}

impl ReplicationService {
    pub fn new(storage: Arc<dyn ReplicationStorage>) -> Self {
        Self {
            storage,
            start_time: Instant::now(),
            pending_prepares: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// WriteReplica.WriteObject - Direct write (non-2PC).
    pub async fn write_object(&self, req: proto::WriteObjectRequest) -> proto::WriteObjectResponse {
        let key = &req.key;
        let data = &req.data;
        let expected_checksum = &req.checksum;

        // Verify checksum
        if !expected_checksum.is_empty() {
            let actual = compute_sha256(data);
            if actual != *expected_checksum {
                return proto::WriteObjectResponse {
                    success: false,
                    error_message: format!(
                        "checksum mismatch: expected {}, got {}",
                        expected_checksum, actual
                    ),
                    bytes_written: 0,
                };
            }
        }

        match self.storage.put_object(key, data.as_slice()).await {
            Ok(()) => proto::WriteObjectResponse {
                success: true,
                error_message: String::new(),
                bytes_written: data.len() as u64,
            },
            Err(e) => proto::WriteObjectResponse {
                success: false,
                error_message: e.to_string(),
                bytes_written: 0,
            },
        }
    }

    /// WriteReplica.PrepareObject - 2PC phase 1: write to temp.
    pub async fn prepare_object(
        &self,
        req: proto::PrepareObjectRequest,
    ) -> proto::PrepareObjectResponse {
        let key = &req.key;
        let data = &req.data;
        let expected_checksum = &req.checksum;

        // Verify checksum
        if !expected_checksum.is_empty() {
            let actual = compute_sha256(data);
            if actual != *expected_checksum {
                return proto::PrepareObjectResponse {
                    success: false,
                    error_message: format!(
                        "checksum mismatch: expected {}, got {}",
                        expected_checksum, actual
                    ),
                    temp_id: String::new(),
                };
            }
        }

        match self.storage.write_temp_object(key, data.as_slice()).await {
            Ok(temp_object) => {
                let temp_id = format!("{}:{}", req.request_id, key);
                let pending = PendingPrepare {
                    temp_object,
                    checksum: expected_checksum.clone(),
                    created_at: Instant::now(),
                };

                self.pending_prepares
                    .write()
                    .await
                    .insert(temp_id.clone(), pending);

                debug!(key = %key, temp_id = %temp_id, "Prepared object");

                proto::PrepareObjectResponse {
                    success: true,
                    error_message: String::new(),
                    temp_id,
                }
            }
            Err(e) => proto::PrepareObjectResponse {
                success: false,
                error_message: e.to_string(),
                temp_id: String::new(),
            },
        }
    }

    /// WriteReplica.StreamPrepareObject - Streaming 2PC phase 1 for large objects.
    pub async fn stream_prepare_object(
        &self,
        chunks: Vec<proto::PrepareChunkRequest>,
    ) -> proto::PrepareObjectResponse {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncWriteExt;

        if chunks.is_empty() {
            return proto::PrepareObjectResponse {
                success: false,
                error_message: "no chunks received".to_string(),
                temp_id: String::new(),
            };
        }

        // Extract key and request_id from first chunk
        let first_chunk = &chunks[0];
        let key = &first_chunk.key;
        let request_id = first_chunk.request_id;

        if key.is_empty() {
            return proto::PrepareObjectResponse {
                success: false,
                error_message: "missing key in first chunk".to_string(),
                temp_id: String::new(),
            };
        }

        // Create TempObject handle (this gives us the proper temp path without creating a file)
        let temp_object = match self.storage.create_temp_object(key).await {
            Ok(t) => t,
            Err(e) => {
                return proto::PrepareObjectResponse {
                    success: false,
                    error_message: format!("failed to create temp object: {}", e),
                    temp_id: String::new(),
                };
            }
        };

        let temp_path = temp_object.temp_path();

        // Create file and write chunks directly to the temp path
        let mut file = match tokio::fs::File::create(temp_path).await {
            Ok(f) => f,
            Err(e) => {
                return proto::PrepareObjectResponse {
                    success: false,
                    error_message: format!("failed to create temp file: {}", e),
                    temp_id: String::new(),
                };
            }
        };

        let mut hasher = Sha256::new();
        let mut expected_checksum = String::new();

        for chunk in &chunks {
            if !chunk.chunk.is_empty() {
                hasher.update(&chunk.chunk);
                if let Err(e) = file.write_all(&chunk.chunk).await {
                    // temp_object will clean up on drop
                    return proto::PrepareObjectResponse {
                        success: false,
                        error_message: format!("failed to write chunk: {}", e),
                        temp_id: String::new(),
                    };
                }
            }
            if chunk.is_last && !chunk.checksum.is_empty() {
                expected_checksum = chunk.checksum.clone();
            }
        }

        if let Err(e) = file.flush().await {
            return proto::PrepareObjectResponse {
                success: false,
                error_message: format!("failed to flush: {}", e),
                temp_id: String::new(),
            };
        }
        drop(file);

        // Verify checksum if provided
        if !expected_checksum.is_empty() {
            let actual = hex::encode(hasher.finalize());
            if actual != expected_checksum {
                return proto::PrepareObjectResponse {
                    success: false,
                    error_message: format!(
                        "checksum mismatch: expected {}, got {}",
                        expected_checksum, actual
                    ),
                    temp_id: String::new(),
                };
            }
        }

        let temp_id = format!("{}:{}", request_id, key);
        let pending = PendingPrepare {
            temp_object,
            checksum: expected_checksum,
            created_at: Instant::now(),
        };

        self.pending_prepares
            .write()
            .await
            .insert(temp_id.clone(), pending);

        debug!(key = %key, temp_id = %temp_id, "Stream prepared object");

        proto::PrepareObjectResponse {
            success: true,
            error_message: String::new(),
            temp_id,
        }
    }

    /// WriteReplica.CommitObject - 2PC phase 2: rename temp to final.
    pub async fn commit_object(
        &self,
        req: proto::CommitObjectRequest,
    ) -> proto::CommitObjectResponse {
        let pending = self.pending_prepares.write().await.remove(&req.temp_id);

        match pending {
            Some(p) => match self.storage.commit_object(p.temp_object).await {
                Ok(()) => {
                    debug!(temp_id = %req.temp_id, "Committed object");
                    proto::CommitObjectResponse {
                        success: true,
                        error_message: String::new(),
                    }
                }
                Err(e) => proto::CommitObjectResponse {
                    success: false,
                    error_message: e.to_string(),
                },
            },
            None => proto::CommitObjectResponse {
                success: false,
                error_message: format!("no pending prepare for temp_id: {}", req.temp_id),
            },
        }
    }

    /// WriteReplica.AbortObject - 2PC rollback: cleanup temp.
    pub async fn abort_object(&self, req: proto::AbortObjectRequest) -> proto::AbortObjectResponse {
        let pending = self.pending_prepares.write().await.remove(&req.temp_id);

        match pending {
            Some(p) => {
                // TempObject auto-cleans up on drop
                drop(p.temp_object);
                debug!(temp_id = %req.temp_id, "Aborted object");
                proto::AbortObjectResponse {
                    success: true,
                    error_message: String::new(),
                }
            }
            None => {
                // Idempotent: already aborted or never prepared
                proto::AbortObjectResponse {
                    success: true,
                    error_message: String::new(),
                }
            }
        }
    }

    /// ReadReplica.ReadObject - Stream object data in chunks.
    /// Returns Err for not found, Ok with chunks for success.
    pub async fn read_object(
        &self,
        req: proto::ReadObjectRequest,
    ) -> Result<Vec<proto::ReadObjectResponse>, crate::StorageError> {
        let key = &req.key;
        let mut file = self.storage.get_object(key).await?;
        let mut chunks = Vec::new();
        let mut buf = vec![0u8; 64 * 1024]; // 64KB chunks
        let mut total_bytes = 0u64;

        loop {
            match file.read(&mut buf).await {
                Ok(0) => {
                    if chunks.is_empty() {
                        chunks.push(proto::ReadObjectResponse {
                            chunk: Vec::new(),
                            is_last: true,
                            total_size: 0,
                        });
                    }
                    break;
                }
                Ok(n) => {
                    total_bytes += n as u64;
                    chunks.push(proto::ReadObjectResponse {
                        chunk: buf[..n].to_vec(),
                        is_last: false,
                        total_size: 0,
                    });
                }
                Err(e) => {
                    warn!(key = %key, error = %e, "Read error");
                    break;
                }
            }
        }

        // Set total_size in first chunk and is_last in last chunk
        if let Some(first_chunk) = chunks.first_mut() {
            first_chunk.total_size = total_bytes;
        }
        if let Some(last_chunk) = chunks.last_mut() {
            last_chunk.is_last = true;
        }

        Ok(chunks)
    }

    /// ReadReplica.ObjectExists - Check if object exists.
    pub async fn object_exists(
        &self,
        req: proto::ObjectExistsRequest,
    ) -> proto::ObjectExistsResponse {
        match self.storage.object_info(&req.key).await {
            Ok((size, checksum)) => proto::ObjectExistsResponse {
                exists: true,
                size,
                checksum,
            },
            Err(_) => proto::ObjectExistsResponse {
                exists: false,
                size: 0,
                checksum: String::new(),
            },
        }
    }

    /// DeleteReplica.DeleteObject - Delete object.
    pub async fn delete_object(
        &self,
        req: proto::DeleteObjectRequest,
    ) -> proto::DeleteObjectResponse {
        // Check if exists first for was_present
        let was_present = self.storage.get_object(&req.key).await.is_ok();

        match self.storage.delete_object(&req.key).await {
            Ok(()) => proto::DeleteObjectResponse {
                success: true,
                error_message: String::new(),
                was_present,
            },
            Err(crate::StorageError::NotFound(_)) => proto::DeleteObjectResponse {
                success: true,
                error_message: String::new(),
                was_present: false,
            },
            Err(e) => proto::DeleteObjectResponse {
                success: false,
                error_message: e.to_string(),
                was_present,
            },
        }
    }

    /// ReplicationHealth.HealthCheck - Return node health status.
    pub fn health_check(&self) -> proto::HealthCheckResponse {
        proto::HealthCheckResponse {
            status: proto::health_check_response::Status::Healthy as i32,
            message: String::new(),
            uptime_seconds: self.start_time.elapsed().as_secs(),
        }
    }

    /// ReplicationHealth.GetStats - Return storage statistics.
    pub async fn get_stats(&self) -> proto::GetStatsResponse {
        // TODO: Implement actual stats collection
        proto::GetStatsResponse {
            total_objects: 0,
            total_bytes: 0,
            temp_objects: self.pending_prepares.read().await.len() as u64,
            disk_usage_percent: 0.0,
        }
    }

    /// Cleanup stale pending prepares (called periodically).
    pub async fn cleanup_stale_prepares(&self, max_age_secs: u64) {
        let mut prepares = self.pending_prepares.write().await;
        let now = Instant::now();

        prepares.retain(|temp_id, pending| {
            let age = now.duration_since(pending.created_at).as_secs();
            if age > max_age_secs {
                warn!(temp_id = %temp_id, age_secs = %age, "Cleaning up stale prepare");
                false
            } else {
                true
            }
        });
    }
}

fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ObjectStorage;
    use tempfile::TempDir;

    async fn test_service() -> (ReplicationService, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();
        let service = ReplicationService::new(Arc::new(storage));
        (service, temp_dir)
    }

    #[tokio::test]
    async fn test_write_and_read_object() {
        let (service, _dir) = test_service().await;

        let data = b"Hello, World!";
        let checksum = compute_sha256(data);

        let write_resp = service
            .write_object(proto::WriteObjectRequest {
                key: "test/key".to_string(),
                data: data.to_vec(),
                checksum: checksum.clone(),
                request_id: 1,
            })
            .await;

        assert!(write_resp.success);
        assert_eq!(write_resp.bytes_written, data.len() as u64);

        let read_resp = service
            .read_object(proto::ReadObjectRequest {
                key: "test/key".to_string(),
                offset: 0,
                length: 0,
            })
            .await;

        let chunks = read_resp.unwrap();
        assert!(!chunks.is_empty());
        let all_data: Vec<u8> = chunks.iter().flat_map(|r| r.chunk.clone()).collect();
        assert_eq!(all_data, data);
    }

    #[tokio::test]
    async fn test_checksum_mismatch() {
        let (service, _dir) = test_service().await;

        let write_resp = service
            .write_object(proto::WriteObjectRequest {
                key: "test/key".to_string(),
                data: b"data".to_vec(),
                checksum: "wrongchecksum".to_string(),
                request_id: 1,
            })
            .await;

        assert!(!write_resp.success);
        assert!(write_resp.error_message.contains("checksum mismatch"));
    }

    #[tokio::test]
    async fn test_prepare_commit_flow() {
        let (service, _dir) = test_service().await;

        let data = b"test data";
        let checksum = compute_sha256(data);

        let prepare_resp = service
            .prepare_object(proto::PrepareObjectRequest {
                key: "test/key".to_string(),
                data: data.to_vec(),
                checksum,
                request_id: 42,
            })
            .await;

        assert!(prepare_resp.success);
        assert!(!prepare_resp.temp_id.is_empty());

        let commit_resp = service
            .commit_object(proto::CommitObjectRequest {
                key: "test/key".to_string(),
                temp_id: prepare_resp.temp_id,
                request_id: 42,
            })
            .await;

        assert!(commit_resp.success);

        // Verify object is readable
        let exists = service
            .object_exists(proto::ObjectExistsRequest {
                key: "test/key".to_string(),
            })
            .await;

        assert!(exists.exists);
        assert_eq!(exists.size, data.len() as u64);
    }

    #[tokio::test]
    async fn test_prepare_abort_flow() {
        let (service, _dir) = test_service().await;

        let prepare_resp = service
            .prepare_object(proto::PrepareObjectRequest {
                key: "test/key".to_string(),
                data: b"test data".to_vec(),
                checksum: String::new(),
                request_id: 42,
            })
            .await;

        assert!(prepare_resp.success);

        let abort_resp = service
            .abort_object(proto::AbortObjectRequest {
                key: "test/key".to_string(),
                temp_id: prepare_resp.temp_id,
                request_id: 42,
            })
            .await;

        assert!(abort_resp.success);

        // Verify object does not exist
        let exists = service
            .object_exists(proto::ObjectExistsRequest {
                key: "test/key".to_string(),
            })
            .await;

        assert!(!exists.exists);
    }

    #[tokio::test]
    async fn test_delete_object() {
        let (service, _dir) = test_service().await;

        // Write first
        service
            .write_object(proto::WriteObjectRequest {
                key: "test/key".to_string(),
                data: b"data".to_vec(),
                checksum: String::new(),
                request_id: 1,
            })
            .await;

        let delete_resp = service
            .delete_object(proto::DeleteObjectRequest {
                key: "test/key".to_string(),
                request_id: 2,
            })
            .await;

        assert!(delete_resp.success);
        assert!(delete_resp.was_present);

        // Delete non-existent is idempotent
        let delete_resp2 = service
            .delete_object(proto::DeleteObjectRequest {
                key: "test/key".to_string(),
                request_id: 3,
            })
            .await;

        assert!(delete_resp2.success);
        assert!(!delete_resp2.was_present);
    }

    #[tokio::test]
    async fn test_health_check() {
        let (service, _dir) = test_service().await;
        let resp = service.health_check();

        assert_eq!(
            resp.status,
            proto::health_check_response::Status::Healthy as i32
        );
    }
}
