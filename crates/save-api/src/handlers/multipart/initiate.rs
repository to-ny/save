use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{
    InitiateMultipartUploadResult, S3XmlResponse, validate_bucket_name, validate_object_key,
};
use save_metadata::MetadataError;
use std::time::Instant;
use tokio::fs;
use tracing::{debug, info, instrument};
use uuid::Uuid;

use crate::handlers::ApiError;
use crate::state::AppState;

use super::{InitiateQuery, MultipartGaugeGuard, part_path};

#[instrument(skip(state, headers), fields(bucket = %bucket, key = %key))]
pub async fn initiate_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(_query): Query<InitiateQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Initiate multipart upload started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    state
        .metadata
        .get_bucket(&bucket)
        .await
        .map_err(|e| match e {
            MetadataError::BucketNotFound(_) => {
                debug!("Bucket not found: {}", bucket);
                ApiError::BucketNotFound(bucket.clone())
            }
            _ => ApiError::internal(format!("Metadata error: {}", e)),
        })?;

    let upload_id = Uuid::new_v4().to_string();

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let gauge_guard = MultipartGaugeGuard::new();

    state
        .metadata
        .initiate_multipart_upload(&bucket, &key, &upload_id, content_type)
        .await
        .map_err(|e| ApiError::internal(format!("Metadata error: {}", e)))?;

    let parts_dir = part_path(&state.config.storage.data_path, &upload_id, 0)
        .parent()
        .unwrap()
        .to_path_buf();
    fs::create_dir_all(&parts_dir)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

    gauge_guard.disarm();

    let duration = start.elapsed();
    info!(
        "Initiate multipart upload completed in {:?}, upload_id: {}",
        duration, upload_id
    );

    let result = InitiateMultipartUploadResult::new(bucket, key, upload_id);
    let xml = result
        .to_xml()
        .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?;

    crate::metrics::response_size_bytes()
        .with_label_values(&["initiate_multipart"])
        .observe(xml.len() as f64);

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml).into_response())
}
