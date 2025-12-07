use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{ListBucketResult, ListBucketResultV2, S3XmlResponse, validate_bucket_name};
use serde::Deserialize;
use tracing::{info, instrument};

use crate::handlers::{ApiError, ensure_read_consistency, validate_bucket_exists};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListObjectsQuery {
    pub prefix: Option<String>,
    pub marker: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<usize>,
    #[serde(rename = "list-type")]
    pub list_type: Option<String>,
    #[serde(rename = "continuation-token")]
    pub continuation_token: Option<String>,
}

#[instrument(skip(state), fields(bucket = %bucket))]
pub async fn list_objects(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<ListObjectsQuery>,
) -> Result<Response, ApiError> {
    info!("List objects request");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    validate_bucket_exists(&state, &bucket).await?;
    ensure_read_consistency(&state).await?;

    let prefix = params.prefix.as_deref();
    let mut objects = state
        .metadata
        .list_objects(&bucket, prefix)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list objects: {}", e)))?;

    // Check if this is ListObjectsV2 (list-type=2) or v1
    let is_v2 = params.list_type.as_deref() == Some("2");

    // Apply marker/continuation-token filtering
    if is_v2 {
        if let Some(ref token) = params.continuation_token {
            objects.retain(|obj| obj.key > *token);
        }
    } else if let Some(ref marker) = params.marker {
        objects.retain(|obj| obj.key > *marker);
    }

    let max_keys = params.max_keys.unwrap_or(1000).min(1000);
    let is_truncated = objects.len() > max_keys;
    objects.truncate(max_keys);

    let object_list: Vec<(String, _, String, u64)> = objects
        .into_iter()
        .map(|obj| (obj.key, obj.modified_at, obj.etag, obj.size))
        .collect();

    info!(
        "Listed {} objects (v{})",
        object_list.len(),
        if is_v2 { 2 } else { 1 }
    );

    let xml = if is_v2 {
        let result = ListBucketResultV2::new(
            bucket,
            params.prefix,
            params.continuation_token,
            max_keys,
            is_truncated,
            object_list,
        );
        result
            .to_xml()
            .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?
    } else {
        let result = ListBucketResult::new(
            bucket,
            params.prefix,
            params.marker,
            max_keys,
            is_truncated,
            object_list,
        );
        result
            .to_xml()
            .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?
    };

    crate::metrics::response_size_bytes()
        .with_label_values(&["list_objects"])
        .observe(xml.len() as f64);

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml).into_response())
}
