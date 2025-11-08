use axum::{
    response::IntoResponse,
    routing::post,
    Router,
};

use crate::handlers::multipart::{
    complete_multipart, initiate_multipart,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/{bucket}/{*key}",
            post(initiate_or_complete),
        )
}

async fn initiate_or_complete(
    state: axum::extract::State<AppState>,
    path: axum::extract::Path<(String, String)>,
    axum::extract::Query(params): axum::extract::Query<crate::handlers::multipart::MultipartQueryParams>,
    query: axum::extract::Query<crate::handlers::multipart::InitiateQuery>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    if let Some(upload_id) = params.upload_id {
        let query = crate::handlers::multipart::CompleteQuery { upload_id };
        complete_multipart(state, path, axum::extract::Query(query))
            .await
            .into_response()
    } else if query.uploads.is_some() {
        initiate_multipart(state, path, query, headers).await.into_response()
    } else {
        axum::http::StatusCode::BAD_REQUEST.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_route_initiate_multipart() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt?uploads")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_route_complete_multipart() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;

        state
            .metadata
            .initiate_multipart_upload("test-bucket", "file.txt", "test-upload-id", None)
            .await
            .unwrap();
        state
            .metadata
            .record_part("test-bucket", "file.txt", "test-upload-id", 1, "etag".to_string(), 100)
            .await
            .unwrap();

        let app = routes().with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt?uploadId=test-upload-id")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert!(response.status().is_success() || response.status().is_server_error());
    }

    #[tokio::test]
    async fn test_route_invalid_query() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_route_malformed_upload_id() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/test-bucket/file.txt?uploadId=invalid")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
