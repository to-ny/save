mod common;

use axum::{body::Body, http::{Request, StatusCode}};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn test_create_bucket_success() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("PUT", "/my-test-bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["name"], "my-test-bucket");
    assert!(json["created"].is_string());
}

#[tokio::test]
async fn test_create_bucket_duplicate() {
    let (state, _temp_dir) = common::setup_empty().await;

    state
        .metadata
        .create_bucket("existing-bucket")
        .await
        .unwrap();

    let app = save_api::app(state);

    let request = common::request_with_auth("PUT", "/existing-bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_create_bucket_invalid_name_too_short() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("PUT", "/ab", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_bucket_invalid_name_too_long() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let bucket_name = "a".repeat(64);
    let uri = format!("/{}", bucket_name);

    let request = common::request_with_auth("PUT", &uri, Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_bucket_invalid_name_uppercase() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("PUT", "/Invalid-Bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_bucket_invalid_name_special_chars() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("PUT", "/invalid_bucket", Body::empty());

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_create_bucket_missing_auth() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/my-bucket")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_create_bucket_invalid_auth() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/my-bucket")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=wrongkey/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
