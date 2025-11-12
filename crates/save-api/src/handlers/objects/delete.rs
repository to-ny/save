use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tracing::{debug, info, instrument};

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

    // Serialize concurrent operations to the same object
    let _guard = state
        .lock_manager
        .acquire_lock(&bucket, &key)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to acquire object lock: {}", e)))?;

    let object_exists = match state.metadata.get_object_metadata(&bucket, &key).await {
        Ok(_) => true,
        Err(MetadataError::ObjectNotFound { .. }) => {
            debug!("Object not found: {}/{}", bucket, key);
            return Err(ApiError::ObjectNotFound {
                bucket: bucket.clone(),
                key: key.clone(),
            });
        }
        Err(MetadataError::BucketNotFound(_)) => {
            debug!("Bucket not found: {}", bucket);
            return Err(ApiError::BucketNotFound(bucket.clone()));
        }
        Err(e) => {
            return Err(ApiError::internal(format!("Metadata error: {}", e)));
        }
    };

    if object_exists {
        let full_key = storage_key(&bucket, &key);

        // Delete metadata first, then storage. This ensures no phantom objects
        // (metadata pointing to non-existent storage). Orphaned files (if storage
        // deletion fails) are GC'd.
        debug!("Deleting object metadata: {}/{}", bucket, key);
        state
            .metadata
            .delete_object_metadata(&bucket, &key)
            .await
            .map_err(|e| ApiError::internal(format!("Metadata error: {}", e)))?;

        debug!("Deleting object from storage: {}", full_key);
        state
            .storage
            .delete_object(&full_key)
            .await
            .map_err(|e| {
                ApiError::internal(format!(
                    "Storage deletion failed after metadata deletion - orphaned object may require GC: {}",
                    e
                ))
            })?;
    }

    let duration = start.elapsed();
    info!("DELETE completed in {:?}", duration);

    Ok(StatusCode::NO_CONTENT.into_response())
}
