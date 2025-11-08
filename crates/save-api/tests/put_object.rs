use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use serde_json::Value;
use sha2::Digest;
use tempfile::TempDir;
use tower::ServiceExt;

async fn setup() -> (save_api::AppState, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let data_path = temp_dir.path().join("data");
    let metadata_path = temp_dir.path().join("metadata");

    let mut config = SaveConfig::default();
    config.storage.data_path = data_path.to_str().unwrap().to_string();
    config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

    let storage = ObjectStorage::new(&config.storage.data_path)
        .await
        .unwrap();
    let metadata = MetadataStore::new(&config.storage.metadata_path).unwrap();

    metadata.create_bucket("test-bucket").await.unwrap();

    let state = save_api::AppState::new(storage, metadata, config);
    (state, temp_dir)
}

#[tokio::test]
async fn test_put_object_success() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert!(json["etag"].is_string());
    let etag = json["etag"].as_str().unwrap();
    assert!(!etag.is_empty());
}

#[tokio::test]
async fn test_put_object_bucket_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/nonexistent-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_put_object_missing_auth() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_put_object_invalid_auth_format() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header("Authorization", "Bearer some-token")
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_put_object_wrong_access_key() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=wrongkey/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_put_object_etag_correctness() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = b"Test content for SHA256";
    let expected_etag = format!("{:x}", sha2::Sha256::digest(content));

    let request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/etag-test.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from(content.to_vec()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let etag = json["etag"].as_str().unwrap();

    assert_eq!(etag, expected_etag);
}

#[tokio::test]
async fn test_put_object_invalid_bucket_name() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/../../etc/passwd/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_put_object_invalid_key() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/path/../../../etc/passwd")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from("Hello, World!"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
