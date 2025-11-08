use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

use super::storage_key;

#[instrument(skip(state), fields(bucket = %bucket, key = %key))]
pub async fn delete_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("DELETE request started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    state
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

    let full_key = storage_key(&bucket, &key);

    debug!("Deleting object from storage: {}", full_key);
    state.storage.delete_object(&full_key).await.map_err(|e| {
        error!("Storage error: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    debug!("Deleting object metadata: {}/{}", bucket, key);
    state
        .metadata
        .delete_object_metadata(&bucket, &key)
        .await
        .map_err(|e| {
            error!("Metadata error: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let duration = start.elapsed();
    info!("DELETE completed in {:?}", duration);

    Ok(StatusCode::NO_CONTENT.into_response())
}
