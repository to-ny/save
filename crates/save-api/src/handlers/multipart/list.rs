use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{ListMultipartUploadsResult, S3XmlResponse, validate_bucket_name};
use save_metadata::MetadataError;
use tracing::{debug, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

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

    let upload_list: Vec<(String, String, _)> = uploads
        .into_iter()
        .map(|u| (u.key, u.upload_id, u.initiated_at))
        .collect();

    info!("Listed {} multipart uploads", upload_list.len());

    let result = ListMultipartUploadsResult::new(bucket, upload_list);
    let xml = result
        .to_xml()
        .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?;

    crate::metrics::response_size_bytes()
        .with_label_values(&["list_multipart"])
        .observe(xml.len() as f64);

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml).into_response())
}
