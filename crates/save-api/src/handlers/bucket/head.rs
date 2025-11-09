use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::validate_bucket_name;
use save_metadata::MetadataError;
use tracing::{debug, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

#[instrument(skip(state), fields(bucket = %bucket))]
pub async fn head_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, ApiError> {
    info!("HEAD bucket request");

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
            _ => ApiError::internal(format!("Failed to check bucket: {}", e)),
        })?;

    info!("Bucket exists");

    Ok((StatusCode::OK, [("content-length", "0")]).into_response())
}
