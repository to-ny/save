use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

#[instrument(skip(state), fields(bucket = %bucket, key = %key))]
pub async fn head_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("HEAD request started");

    validate_bucket_name(&bucket)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    let metadata = state
        .metadata
        .get_object_metadata(&bucket, &key)
        .await
        .map_err(|e| match e {
            MetadataError::ObjectNotFound { .. } | MetadataError::BucketNotFound(_) => {
                debug!("Object not found: {}/{}", bucket, key);
                ApiError::ObjectNotFound(bucket.clone(), key.clone())
            }
            _ => {
                error!("Metadata error: {}", e);
                ApiError::Internal(format!("Metadata error: {}", e))
            }
        })?;

    let last_modified = metadata.modified_at.to_rfc2822();
    let content_type = metadata
        .content_type
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let duration = start.elapsed();
    info!(
        "HEAD completed in {:?}, size: {} bytes, etag: {}",
        duration, metadata.size, metadata.etag
    );

    Ok((
        StatusCode::OK,
        [
            (header::ETAG, format!("\"{}\"", metadata.etag)),
            (header::CONTENT_LENGTH, metadata.size.to_string()),
            (header::LAST_MODIFIED, last_modified),
            (header::CONTENT_TYPE, content_type),
        ],
    )
        .into_response())
}
