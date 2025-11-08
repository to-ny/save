use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use save_common::validate_bucket_name;
use save_metadata::MetadataError;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListObjectsResponse {
    pub name: String,
    pub prefix: Option<String>,
    pub marker: Option<String>,
    pub max_keys: Option<usize>,
    pub is_truncated: bool,
    pub contents: Vec<ObjectInfo>,
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

    let object_infos: Vec<ObjectInfo> = objects
        .into_iter()
        .map(|obj| ObjectInfo {
            key: obj.key,
            size: obj.size,
            etag: obj.etag,
            last_modified: obj.modified_at,
        })
        .collect();

    info!("Listed {} objects", object_infos.len());

    Ok((
        StatusCode::OK,
        Json(ListObjectsResponse {
            name: bucket,
            prefix: params.prefix,
            marker: params.marker,
            max_keys: Some(max_keys),
            is_truncated,
            contents: object_infos,
        }),
    )
        .into_response())
}
