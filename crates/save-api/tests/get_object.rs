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
async fn test_get_object_success() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = b"Hello, World!";

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

    let get_request = Request::builder()
        .method("GET")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let get_response = app.oneshot(get_request).await.unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);

    let headers = get_response.headers();
    assert!(headers.contains_key("etag"));
    assert!(headers.contains_key("content-length"));
    assert!(headers.contains_key("last-modified"));
    assert_eq!(
        headers.get("content-type").unwrap(),
        "application/octet-stream"
    );
    assert_eq!(headers.get("content-length").unwrap(), "13");

    let body = get_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), content);
}

#[tokio::test]
async fn test_get_object_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("GET")
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
async fn test_get_object_bucket_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("GET")
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
async fn test_get_object_missing_auth() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("GET")
        .uri("/test-bucket/test-file.txt")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_get_object_etag_matches_put() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = b"Test content for ETag verification";

    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/etag-test.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from(content.to_vec()))
        .unwrap();

    let put_response = app.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let put_body = put_response.into_body().collect().await.unwrap().to_bytes();
    let put_json: serde_json::Value = serde_json::from_slice(&put_body).unwrap();
    let put_etag = put_json["etag"].as_str().unwrap();

    let get_request = Request::builder()
        .method("GET")
        .uri("/test-bucket/etag-test.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let get_response = app.oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let get_etag = get_response
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap();

    assert_eq!(get_etag, format!("\"{}\"", put_etag));
}

#[tokio::test]
async fn test_get_object_large_file() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let content = vec![0xAB; 100_000];

    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/large-file.bin")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from(content.clone()))
        .unwrap();

    let put_response = app.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let get_request = Request::builder()
        .method("GET")
        .uri("/test-bucket/large-file.bin")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let get_response = app.oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let headers = get_response.headers();
    assert_eq!(headers.get("content-length").unwrap(), "100000");

    let body = get_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 100_000);
    assert_eq!(body.as_ref(), content.as_slice());
}

#[tokio::test]
async fn test_get_object_invalid_bucket_name() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("GET")
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
async fn test_get_object_invalid_key() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("GET")
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
