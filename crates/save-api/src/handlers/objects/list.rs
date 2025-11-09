use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{ListBucketResult, S3XmlResponse, validate_bucket_name};
use save_metadata::MetadataError;
use serde::Deserialize;
use tracing::{debug, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListObjectsQuery {
    pub prefix: Option<String>,
    pub marker: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<usize>,
}

#[instrument(skip(state), fields(bucket = %bucket))]
pub async fn list_objects(
    State(state): State<AppState>,
    Path(bucket): Path<String>,
    Query(params): Query<ListObjectsQuery>,
) -> Result<Response, ApiError> {
    info!("List objects request");

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

    let prefix = params.prefix.as_deref();
    let mut objects = state
        .metadata
        .list_objects(&bucket, prefix)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to list objects: {}", e)))?;

    if let Some(ref marker) = params.marker {
        objects.retain(|obj| obj.key > *marker);
    }

    let max_keys = params.max_keys.unwrap_or(1000).min(1000);
    let is_truncated = objects.len() > max_keys;
    objects.truncate(max_keys);

    let object_list: Vec<(String, _, String, u64)> = objects
        .into_iter()
        .map(|obj| (obj.key, obj.modified_at, obj.etag, obj.size))
        .collect();

    info!("Listed {} objects", object_list.len());

    let result = ListBucketResult::new(
        bucket,
        params.prefix,
        params.marker,
        max_keys,
        is_truncated,
        object_list,
    );
    let xml = result
        .to_xml()
        .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?;

    crate::metrics::response_size_bytes()
        .with_label_values(&["list_objects"])
        .observe(xml.len() as f64);

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml).into_response())
}
