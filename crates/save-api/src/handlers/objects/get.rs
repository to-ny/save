use axum::{
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tokio_util::io::ReaderStream;
use tracing::{debug, info, instrument};

use crate::handlers::ApiError;
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
            (header::ETAG, format!("\"{}\"", metadata.etag)),
            (header::CONTENT_LENGTH, metadata.size.to_string()),
            (header::LAST_MODIFIED, last_modified),
            (header::CONTENT_TYPE, content_type),
        ],
        body,
    )
        .into_response())
}
