mod common;

use axum::{body::Body, http::StatusCode};
use common::{get_all_elements, get_element_text, parse_xml};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod create {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn test_create_bucket_success() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("PUT", "/my-test-bucket", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // S3 CreateBucket returns an empty body with a Location header
        let headers = response.headers();
        assert_eq!(headers.get("location").unwrap(), "/my-test-bucket");
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

mod delete {
    use super::*;
    use axum::http::Request;

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
        let (state, _temp_dir) = common::setup_empty().await;

        state.metadata.create_bucket("test-bucket").await.unwrap();

        let app = save_api::app(state);

        let request = Request::builder()
            .method("DELETE")
            .uri("/test-bucket")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_delete_bucket_invalid_auth() {
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

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
}

mod list {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn test_list_buckets_success() {
        let (state, _temp_dir) = common::setup_empty().await;

        state.metadata.create_bucket("bucket-a").await.unwrap();
        state.metadata.create_bucket("bucket-b").await.unwrap();
        state.metadata.create_bucket("bucket-c").await.unwrap();

        let app = save_api::app(state);
        let request = common::request_with_auth("GET", "/", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListAllMyBucketsResult");
        assert!(body_str.contains("xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\""));

        assert!(get_element_text(root, "ID").is_some());
        assert!(get_element_text(root, "DisplayName").is_some());

        let bucket_elements = get_all_elements(root, "Bucket");
        assert_eq!(bucket_elements.len(), 3);

        let bucket_names: Vec<_> = bucket_elements
            .iter()
            .filter_map(|b| get_element_text(*b, "Name"))
            .collect();

        assert!(bucket_names.contains(&"bucket-a"));
        assert!(bucket_names.contains(&"bucket-b"));
        assert!(bucket_names.contains(&"bucket-c"));

        for bucket in &bucket_elements {
            assert!(
                get_element_text(*bucket, "CreationDate").is_some(),
                "Bucket missing CreationDate"
            );
        }
    }

    #[tokio::test]
    async fn test_list_buckets_empty() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("GET", "/", Body::empty());
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListAllMyBucketsResult");
        assert!(body_str.contains("xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\""));

        let bucket_elements = get_all_elements(root, "Bucket");
        assert_eq!(bucket_elements.len(), 0);
    }

    #[tokio::test]
    async fn test_list_buckets_missing_auth() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = Request::builder()
            .method("GET")
            .uri("/")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

mod head {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn test_head_bucket_exists() {
        let (state, _temp_dir) = common::setup_empty().await;

        state.metadata.create_bucket("test-bucket").await.unwrap();

        let app = save_api::app(state);

        let request = common::request_with_auth("HEAD", "/test-bucket", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-length").unwrap(), "0");
    }

    #[tokio::test]
    async fn test_head_bucket_not_found() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("HEAD", "/nonexistent-bucket", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_head_bucket_invalid_name() {
        let (state, _temp_dir) = common::setup_empty().await;
        let app = save_api::app(state);

        let request = common::request_with_auth("HEAD", "/Invalid_Bucket", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_head_bucket_missing_auth() {
        let (state, _temp_dir) = common::setup_empty().await;

        state.metadata.create_bucket("test-bucket").await.unwrap();

        let app = save_api::app(state);

        let request = Request::builder()
            .method("HEAD")
            .uri("/test-bucket")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
