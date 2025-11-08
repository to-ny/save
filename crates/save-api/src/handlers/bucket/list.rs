use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::{error, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct BucketInfo {
    pub name: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListBucketsResponse {
    pub buckets: Vec<BucketInfo>,
}

#[instrument(skip(state))]
pub async fn list_buckets(State(state): State<AppState>) -> Result<Response, ApiError> {
    info!("List buckets request");

    let buckets = state.metadata.list_buckets().await.map_err(|e| {
        error!("Failed to list buckets: {}", e);
        ApiError::Internal(format!("Failed to list buckets: {}", e))
    })?;

    let bucket_infos: Vec<BucketInfo> = buckets
        .into_iter()
        .map(|b| BucketInfo {
            name: b.name,
            created: b.created_at,
        })
        .collect();

    info!("Listed {} buckets", bucket_infos.len());

    Ok((
        StatusCode::OK,
        Json(ListBucketsResponse {
            buckets: bucket_infos,
        }),
    )
        .into_response())
}
