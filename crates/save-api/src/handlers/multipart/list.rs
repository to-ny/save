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

#[derive(Debug, Clone, Serialize)]
pub struct UploadInfo {
    pub upload_id: String,
    pub key: String,
    pub initiated: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListUploadsResponse {
    pub bucket: String,
    pub uploads: Vec<UploadInfo>,
}

#[instrument(skip(state), fields(bucket = %bucket))]
pub async fn list_multipart_uploads(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
) -> Result<Response, ApiError> {
    info!("List multipart uploads request");

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
            _ => ApiError::internal(format!("Failed to get bucket: {}", e)),
        })?;

    let uploads = state
        .metadata
        .list_multipart_uploads(&bucket)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list multipart uploads: {}", e)))?;

    let upload_infos: Vec<UploadInfo> = uploads
        .into_iter()
        .map(|u| UploadInfo {
            upload_id: u.upload_id,
            key: u.key,
            initiated: u.initiated_at,
        })
        .collect();

    info!("Listed {} multipart uploads", upload_infos.len());

    Ok((
        StatusCode::OK,
        Json(ListUploadsResponse {
            bucket,
            uploads: upload_infos,
        }),
    )
        .into_response())
}
