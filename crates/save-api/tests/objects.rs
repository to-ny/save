mod common;

use axum::{body::Body, http::StatusCode};
use http_body_util::BodyExt;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use serde_json::Value;
use sha2::Digest;
use tempfile::TempDir;
use tower::ServiceExt;

mod delete {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let data_path = temp_dir.path().join("data");
        let metadata_path = temp_dir.path().join("metadata");

        let mut config = SaveConfig::default();
        config.storage.data_path = data_path.to_str().unwrap().to_string();
        config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

        let storage = ObjectStorage::new(&config.storage.data_path).await.unwrap();
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
}

mod get {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let data_path = temp_dir.path().join("data");
        let metadata_path = temp_dir.path().join("metadata");

        let mut config = SaveConfig::default();
        config.storage.data_path = data_path.to_str().unwrap().to_string();
        config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

        let storage = ObjectStorage::new(&config.storage.data_path).await.unwrap();
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
}

mod head {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let data_path = temp_dir.path().join("data");
        let metadata_path = temp_dir.path().join("metadata");

        let mut config = SaveConfig::default();
        config.storage.data_path = data_path.to_str().unwrap().to_string();
        config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

        let storage = ObjectStorage::new(&config.storage.data_path).await.unwrap();
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

        let body = head_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
        assert_eq!(
            head_headers.get("content-length"),
            get_headers.get("content-length")
        );
        assert_eq!(
            head_headers.get("last-modified"),
            get_headers.get("last-modified")
        );
        assert_eq!(
            head_headers.get("content-type"),
            get_headers.get("content-type")
        );

        let head_body = head_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
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

        let body = head_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(body.len(), 0);
    }
}

mod list {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn test_list_objects_success() {
        let (state, _temp_dir) = common::setup().await;

        let obj1 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "file1.txt".to_string(),
            100,
            "etag1".to_string(),
        );
        let obj2 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "file2.txt".to_string(),
            200,
            "etag2".to_string(),
        );
        let obj3 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "dir/file3.txt".to_string(),
            300,
            "etag3".to_string(),
        );

        state.metadata.put_object_metadata(obj1).await.unwrap();
        state.metadata.put_object_metadata(obj2).await.unwrap();
        state.metadata.put_object_metadata(obj3).await.unwrap();

        let app = save_api::app(state);
        let request = common::request_with_auth("GET", "/test-bucket", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["name"], "test-bucket");
        assert_eq!(json["contents"].as_array().unwrap().len(), 3);
        assert_eq!(json["is_truncated"], false);

        for object in json["contents"].as_array().unwrap() {
            assert!(object["key"].is_string());
            assert!(object["size"].is_number());
            assert!(object["etag"].is_string());
            assert!(object["last_modified"].is_string());
        }
    }

    #[tokio::test]
    async fn test_list_objects_empty_bucket() {
        let (state, _temp_dir) = common::setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("GET", "/test-bucket", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["name"], "test-bucket");
        assert_eq!(json["contents"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_list_objects_with_prefix() {
        let (state, _temp_dir) = common::setup().await;

        let obj1 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "file1.txt".to_string(),
            100,
            "etag1".to_string(),
        );
        let obj2 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "dir/file2.txt".to_string(),
            200,
            "etag2".to_string(),
        );
        let obj3 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "dir/file3.txt".to_string(),
            300,
            "etag3".to_string(),
        );

        state.metadata.put_object_metadata(obj1).await.unwrap();
        state.metadata.put_object_metadata(obj2).await.unwrap();
        state.metadata.put_object_metadata(obj3).await.unwrap();

        let app = save_api::app(state);
        let request = common::request_with_auth("GET", "/test-bucket?prefix=dir/", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["prefix"], "dir/");
        assert_eq!(json["contents"].as_array().unwrap().len(), 2);

        for object in json["contents"].as_array().unwrap() {
            let key = object["key"].as_str().unwrap();
            assert!(key.starts_with("dir/"));
        }
    }

    #[tokio::test]
    async fn test_list_objects_with_marker() {
        let (state, _temp_dir) = common::setup().await;

        let obj1 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "a-file.txt".to_string(),
            100,
            "etag1".to_string(),
        );
        let obj2 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "b-file.txt".to_string(),
            200,
            "etag2".to_string(),
        );
        let obj3 = save_metadata::ObjectMetadata::new(
            "test-bucket".to_string(),
            "c-file.txt".to_string(),
            300,
            "etag3".to_string(),
        );

        state.metadata.put_object_metadata(obj1).await.unwrap();
        state.metadata.put_object_metadata(obj2).await.unwrap();
        state.metadata.put_object_metadata(obj3).await.unwrap();

        let app = save_api::app(state);
        let request =
            common::request_with_auth("GET", "/test-bucket?marker=a-file.txt", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["marker"], "a-file.txt");

        let objects = json["contents"].as_array().unwrap();
        for object in objects {
            let key = object["key"].as_str().unwrap();
            assert!(key > "a-file.txt");
        }
    }

    #[tokio::test]
    async fn test_list_objects_with_max_keys() {
        let (state, _temp_dir) = common::setup().await;

        for i in 0..10 {
            let key = format!("file{}.txt", i);
            let obj = save_metadata::ObjectMetadata::new(
                "test-bucket".to_string(),
                key,
                100,
                "etag".to_string(),
            );
            state.metadata.put_object_metadata(obj).await.unwrap();
        }

        let app = save_api::app(state);
        let request = common::request_with_auth("GET", "/test-bucket?max-keys=5", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["max_keys"], 5);
        assert_eq!(json["contents"].as_array().unwrap().len(), 5);
        assert_eq!(json["is_truncated"], true);
    }

    #[tokio::test]
    async fn test_list_objects_bucket_not_found() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("GET", "/nonexistent-bucket", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_objects_invalid_bucket_name() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("GET", "/Invalid_Bucket", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_list_objects_missing_auth() {
        let (state, _temp_dir) = common::setup().await;
        let app = save_api::app(state);

        let request = Request::builder()
            .method("GET")
            .uri("/test-bucket")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

mod put {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let data_path = temp_dir.path().join("data");
        let metadata_path = temp_dir.path().join("metadata");

        let mut config = SaveConfig::default();
        config.storage.data_path = data_path.to_str().unwrap().to_string();
        config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

        let storage = ObjectStorage::new(&config.storage.data_path).await.unwrap();
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
}
