use axum::{
    Router,
    extract::{Path, Query, State},
    response::Response,
    routing::{get, put},
};
use serde::Deserialize;

use crate::handlers::{
    ApiError,
    bucket::{create_bucket, delete_bucket, head_bucket, list_buckets},
    multipart::list_multipart_uploads,
    objects::{ListObjectsQuery, list_objects},
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
struct BucketListQuery {
    pub prefix: Option<String>,
    pub marker: Option<String>,
    #[serde(rename = "max-keys")]
    pub max_keys: Option<usize>,
    pub uploads: Option<String>,
}

async fn get_bucket_handler(
    state: State<AppState>,
    path: Path<String>,
    Query(params): Query<BucketListQuery>,
) -> Result<Response, ApiError> {
    if params.uploads.is_some() {
        list_multipart_uploads(state, path).await
    } else {
        let objects_query = ListObjectsQuery {
            prefix: params.prefix,
            marker: params.marker,
            max_keys: params.max_keys,
        };
        list_objects(state, path, Query(objects_query)).await
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/", get(list_buckets)).route(
        "/{bucket}",
        put(create_bucket)
            .delete(delete_bucket)
            .get(get_bucket_handler)
            .head(head_bucket),
    )
}
