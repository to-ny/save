mod common;

use axum::{body::Body, http::StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn test_delete_bucket_success() {
    let (state, _temp_dir) = common::setup_empty().await;

    state.metadata.create_bucket("empty-bucket").await.unwrap();

    let app = save_api::app(state);

    let request = common::request_with_auth("DELETE", "/empty-bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_delete_bucket_not_empty() {
    let (state, _temp_dir) = common::setup_empty().await;

    state.metadata.create_bucket("full-bucket").await.unwrap();

    let object_metadata = save_metadata::ObjectMetadata::new(
        "full-bucket".to_string(),
        "test-file.txt".to_string(),
        100,
        "etag123".to_string(),
    );
    state
        .metadata
        .put_object_metadata(object_metadata)
        .await
        .unwrap();

    let app = save_api::app(state);

    let request = common::request_with_auth("DELETE", "/full-bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_delete_bucket_not_found() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("DELETE", "/nonexistent-bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_bucket_invalid_name() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("DELETE", "/Invalid_Bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_delete_bucket_missing_auth() {
    use axum::http::Request;

    let (state, _temp_dir) = common::setup_empty().await;

    state.metadata.create_bucket("test-bucket").await.unwrap();

    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
        .uri("/test-bucket")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_delete_bucket_invalid_auth() {
    use axum::http::Request;

    let (state, _temp_dir) = common::setup_empty().await;

    state.metadata.create_bucket("test-bucket").await.unwrap();

    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
        .uri("/test-bucket")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=wrongkey/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_delete_bucket_with_multipart_uploads() {
    let (state, _temp_dir) = common::setup_empty().await;

    state.metadata.create_bucket("mp-bucket").await.unwrap();

    state
        .metadata
        .initiate_multipart_upload("mp-bucket", "test-file.txt", "upload-123", None)
        .await
        .unwrap();

    let app = save_api::app(state);

    let request = common::request_with_auth("DELETE", "/mp-bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_delete_bucket_idempotency() {
    let (state, _temp_dir) = common::setup_empty().await;

    state.metadata.create_bucket("test-bucket").await.unwrap();

    let app = save_api::app(state.clone());

    let request1 = common::request_with_auth("DELETE", "/test-bucket", Body::empty());
    let response1 = app.oneshot(request1).await.unwrap();
    assert_eq!(response1.status(), StatusCode::NO_CONTENT);

    let app2 = save_api::app(state);
    let request2 = common::request_with_auth("DELETE", "/test-bucket", Body::empty());
    let response2 = app2.oneshot(request2).await.unwrap();
    assert_eq!(response2.status(), StatusCode::NOT_FOUND);
}
