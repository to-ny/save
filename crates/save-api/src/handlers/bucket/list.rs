use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{ListAllMyBucketsResult, S3XmlResponse};
use tracing::{info, instrument};

use crate::handlers::{ApiError, ensure_read_consistency};
use crate::state::AppState;

#[instrument(skip(state))]
pub async fn list_buckets(State(state): State<AppState>) -> Result<Response, ApiError> {
    info!("List buckets request");

    ensure_read_consistency(&state).await?;

    let buckets = state
        .metadata
        .list_buckets()
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list buckets: {}", e)))?;

    let bucket_list: Vec<(String, _)> = buckets
        .into_iter()
        .map(|b| (b.name, b.created_at))
        .collect();

    info!("Listed {} buckets", bucket_list.len());

    // TODO: Extract owner_id from auth context
    let owner_id = "test-access-key".to_string();
    let result = ListAllMyBucketsResult::new(bucket_list, owner_id);
    let xml = result
        .to_xml()
        .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?;

    crate::metrics::response_size_bytes()
        .with_label_values(&["list_buckets"])
        .observe(xml.len() as f64);

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml).into_response())
}
