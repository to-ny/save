use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::config::ConsistencyMode;
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::{MetadataError, lock::DistributedReadLockGuard};
use std::time::Instant;
use tokio_util::io::ReaderStream;
use tracing::{debug, info, instrument};

use crate::handlers::{ApiError, ensure_read_consistency};
use crate::state::AppState;

use super::storage_key;

#[instrument(skip(state), fields(bucket = %bucket, key = %key))]
pub async fn get_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("GET request started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    // In eventual consistency mode, reads are served locally without distributed locking.
    // This allows reads to continue even when the Raft leader is unavailable.
    // In strong consistency mode, we acquire a distributed read lock to ensure
    // linearizable reads (no stale data during concurrent writes).
    let _guard: Option<DistributedReadLockGuard> = if state.config.cluster.consistency_mode
        == ConsistencyMode::Strong
    {
        Some(
            state
                .lock_manager
                .acquire_read_lock(&bucket, &key)
                .await
                .map_err(|e| ApiError::internal(format!("Failed to acquire object lock: {}", e)))?,
        )
    } else {
        None
    };

    ensure_read_consistency(&state).await?;

    let metadata = state
        .metadata
        .get_object_metadata(&bucket, &key)
        .await
        .map_err(|e| match e {
            MetadataError::ObjectNotFound { .. } | MetadataError::BucketNotFound(_) => {
                debug!("Object not found: {}/{}", bucket, key);
                ApiError::ObjectNotFound {
                    bucket: bucket.clone(),
                    key: key.clone(),
                }
            }
            _ => ApiError::internal(format!("Metadata error: {}", e)),
        })?;

    let full_key = storage_key(&bucket, &key);

    debug!("Reading object from storage: {}", full_key);
    let file = state
        .storage
        .get_object(&full_key)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let last_modified = metadata
        .modified_at
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string();
    let content_type = metadata
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let duration = start.elapsed();
    info!(
        "GET completed in {:?}, size: {} bytes, etag: {}",
        duration, metadata.size, metadata.etag
    );

    Ok((
        StatusCode::OK,
        [
            ("etag", format!("\"{}\"", metadata.etag)),
            ("content-length", metadata.size.to_string()),
            ("last-modified", last_modified),
            ("content-type", content_type),
            ("x-storage-duration-ms", duration.as_millis().to_string()),
        ],
        body,
    )
        .into_response())
}
