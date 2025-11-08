use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tokio::fs;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::metrics::multipart_uploads_in_progress;
use crate::state::AppState;

use super::{CompleteQuery, part_path};

#[instrument(skip(state), fields(bucket = %bucket, key = %key, upload_id = %query.upload_id))]
pub async fn abort_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<CompleteQuery>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Abort multipart upload started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    state
        .metadata
        .get_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| match e {
            MetadataError::MultipartUploadNotFound(_) => {
                debug!("Multipart upload not found: {}", query.upload_id);
                ApiError::InvalidRequest(format!("Upload ID not found: {}", query.upload_id))
            }
            _ => {
                error!("Metadata error: {}", e);
                ApiError::Internal(format!("Metadata error: {}", e))
            }
        })?;

    state
        .metadata
        .abort_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| {
            error!("Failed to abort multipart upload: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let parts_dir = part_path(&state.config.storage.data_path, &query.upload_id, 0)
        .parent()
        .unwrap()
        .to_path_buf();
    if let Err(e) = fs::remove_dir_all(&parts_dir).await {
        error!("Failed to clean up part files: {}", e);
    }

    multipart_uploads_in_progress().dec();

    let duration = start.elapsed();
    info!("Abort multipart upload completed in {:?}", duration);

    Ok(StatusCode::NO_CONTENT.into_response())
}
