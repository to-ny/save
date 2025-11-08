use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tokio::fs;
use tracing::{debug, error, info, instrument};
use uuid::Uuid;

use crate::handlers::ApiError;
use crate::state::AppState;

use super::{InitiateQuery, InitiateResponse, MultipartGaugeGuard, part_path};

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
            _ => {
                error!("Metadata error: {}", e);
                ApiError::Internal(format!("Metadata error: {}", e))
            }
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
        .map_err(|e| {
            error!("Failed to initiate multipart upload: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let parts_dir = part_path(&state.config.storage.data_path, &upload_id, 0)
        .parent()
        .unwrap()
        .to_path_buf();
    fs::create_dir_all(&parts_dir).await.map_err(|e| {
        error!("Failed to create parts directory: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    gauge_guard.disarm();

    let duration = start.elapsed();
    info!(
        "Initiate multipart upload completed in {:?}, upload_id: {}",
        duration, upload_id
    );

    Ok((StatusCode::OK, Json(InitiateResponse { upload_id })).into_response())
}
