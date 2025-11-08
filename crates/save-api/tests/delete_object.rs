use axum::{
    body::Body,
    http::{Request, StatusCode},
};
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
async fn test_delete_object_success() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let put_request = Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::from("Hello, World!"))
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

    let get_response = app.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let delete_request = Request::builder()
        .method("DELETE")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let delete_response = app.clone().oneshot(delete_request).await.unwrap();
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

    let get_after_delete = Request::builder()
        .method("GET")
        .uri("/test-bucket/test-file.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let get_response_after = app.oneshot(get_after_delete).await.unwrap();
    assert_eq!(get_response_after.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_object_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
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
async fn test_delete_object_bucket_not_found() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
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
async fn test_delete_object_missing_auth() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
        .uri("/test-bucket/test-file.txt")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_delete_object_idempotency() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let delete_request_1 = Request::builder()
        .method("DELETE")
        .uri("/test-bucket/nonexistent.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response_1 = app.clone().oneshot(delete_request_1).await.unwrap();
    assert_eq!(response_1.status(), StatusCode::NOT_FOUND);

    let delete_request_2 = Request::builder()
        .method("DELETE")
        .uri("/test-bucket/nonexistent.txt")
        .header(
            "Authorization",
            "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
        )
        .body(Body::empty())
        .unwrap();

    let response_2 = app.oneshot(delete_request_2).await.unwrap();
    assert_eq!(response_2.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_delete_object_invalid_bucket_name() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
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
async fn test_delete_object_invalid_key() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    let request = Request::builder()
        .method("DELETE")
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
async fn test_delete_multiple_objects() {
    let (state, _temp_dir) = setup().await;
    let app = save_api::app(state);

    for i in 1..=3 {
        let put_request = Request::builder()
            .method("PUT")
            .uri(format!("/test-bucket/file-{}.txt", i))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::from(format!("Content {}", i)))
            .unwrap();

        let response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    for i in 1..=3 {
        let delete_request = Request::builder()
            .method("DELETE")
            .uri(format!("/test-bucket/file-{}.txt", i))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(delete_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    for i in 1..=3 {
        let get_request = Request::builder()
            .method("GET")
            .uri(format!("/test-bucket/file-{}.txt", i))
            .header(
                "Authorization",
                "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake"
            )
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(get_request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
