use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::validate_bucket_name;
use save_metadata::MetadataError;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

#[instrument(skip(state), fields(bucket = %bucket))]
pub async fn delete_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, ApiError> {
    info!("Delete bucket request");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

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
                error!("Failed to get bucket: {}", e);
                ApiError::Internal(format!("Failed to get bucket: {}", e))
            }
        })?;

    let objects = state
        .metadata
        .list_objects(&bucket, None)
        .await
        .map_err(|e| {
            error!("Failed to list objects: {}", e);
            ApiError::Internal(format!("Failed to list objects: {}", e))
        })?;

    if !objects.is_empty() {
        debug!("Bucket not empty: {} objects found", objects.len());
        return Err(ApiError::BucketNotEmpty(bucket));
    }

    state.metadata.delete_bucket(&bucket).await.map_err(|e| {
        error!("Failed to delete bucket: {}", e);
        ApiError::Internal(format!("Failed to delete bucket: {}", e))
    })?;

    info!("Bucket deleted successfully");

    Ok(StatusCode::NO_CONTENT.into_response())
}
