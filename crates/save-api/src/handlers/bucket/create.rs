use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use save_common::validate_bucket_name;
use save_metadata::MetadataError;
use serde::Serialize;
use tracing::{debug, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct CreateBucketResponse {
    pub name: String,
    pub created: DateTime<Utc>,
}

#[instrument(skip(state), fields(bucket = %bucket))]
pub async fn create_bucket(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, ApiError> {
    info!("Create bucket request");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    let bucket_obj = state
        .metadata
        .create_bucket(&bucket)
        .await
        .map_err(|e| match e {
            MetadataError::BucketAlreadyExists(_) => {
                debug!("Bucket already exists: {}", bucket);
                ApiError::BucketAlreadyExists(bucket.clone())
            }
            _ => ApiError::internal(format!("Failed to create bucket: {}", e)),
        })?;

    info!("Bucket created successfully");

    Ok((
        StatusCode::OK,
        Json(CreateBucketResponse {
            name: bucket_obj.name,
            created: bucket_obj.created_at,
        }),
    )
        .into_response())
}
