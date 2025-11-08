use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
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
async fn test_head_object_success() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = b"Hello, World!";

    // Create an object
    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from(content.to_vec()))
        .unwrap();

    let put_response = app.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let head_request = Request::builder()
        .method("HEAD")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let head_response = app.oneshot(head_request).await.unwrap();

    assert_eq!(head_response.status(), StatusCode::OK);

    let headers = head_response.headers();
    assert!(headers.contains_key("etag"));
    assert!(headers.contains_key("content-length"));
    assert!(headers.contains_key("last-modified"));
    assert_eq!(
        headers.get("content-type").unwrap(),
        "application/octet-stream"
    );
    assert_eq!(headers.get("content-length").unwrap(), "13");

    let body = head_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 0);
}

#[tokio::test]
async fn test_head_object_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("HEAD")
        .uri("/test-bucket/nonexistent.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_head_object_bucket_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("HEAD")
        .uri("/nonexistent-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_head_object_missing_auth() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("HEAD")
        .uri("/test-bucket/test-file.txt")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_head_object_matches_get_headers() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = b"Test content for header comparison";

    // Create an object
    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/header-test.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from(content.to_vec()))
        .unwrap();

    app.clone().oneshot(put_request).await.unwrap();

    let head_request = Request::builder()
        .method("HEAD")
        .uri("/test-bucket/header-test.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let head_response = app.clone().oneshot(head_request).await.unwrap();

    let get_request = Request::builder()
        .method("GET")
        .uri("/test-bucket/header-test.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let get_response = app.oneshot(get_request).await.unwrap();

    let head_headers = head_response.headers();
    let get_headers = get_response.headers();

    assert_eq!(head_headers.get("etag"), get_headers.get("etag"));
    assert_eq!(head_headers.get("content-length"), get_headers.get("content-length"));
    assert_eq!(head_headers.get("last-modified"), get_headers.get("last-modified"));
    assert_eq!(head_headers.get("content-type"), get_headers.get("content-type"));

    let head_body = head_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(head_body.len(), 0);

    let get_body = get_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(get_body.as_ref(), content);
}

#[tokio::test]
async fn test_head_object_invalid_bucket_name() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("HEAD")
        .uri("/../../etc/passwd/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_head_object_invalid_key() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("HEAD")
        .uri("/test-bucket/path/../../../etc/passwd")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_head_object_large_file() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = vec![0xAB; 100_000];

    // Create a large object
    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/large-file.bin")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from(content))
        .unwrap();

    app.clone().oneshot(put_request).await.unwrap();

    let head_request = Request::builder()
        .method("HEAD")
        .uri("/test-bucket/large-file.bin")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let head_response = app.oneshot(head_request).await.unwrap();

    assert_eq!(head_response.status(), StatusCode::OK);

    let headers = head_response.headers();
    assert_eq!(headers.get("content-length").unwrap(), "100000");

    let body = head_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 0);
}
