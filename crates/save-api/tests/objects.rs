mod common;

use axum::{body::Body, http::StatusCode};
use common::{get_all_elements, get_element_text, parse_xml};
use http_body_util::BodyExt;
use sha2::Digest;
use tempfile::TempDir;
use tower::ServiceExt;

mod delete {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        common::setup().await
    }

    #[tokio::test]
    async fn test_delete_object_success() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/test-file.txt",
            b"Hello, World!".to_vec(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_request =
            common::request_with_auth("GET", "/test-bucket/test-file.txt", Body::empty());

        let get_response = app.clone().oneshot(get_request).await.unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);

        let delete_request =
            common::request_with_auth("DELETE", "/test-bucket/test-file.txt", Body::empty());

        let delete_response = app.clone().oneshot(delete_request).await.unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let get_after_delete =
            common::request_with_auth("GET", "/test-bucket/test-file.txt", Body::empty());

        let get_response_after = app.oneshot(get_after_delete).await.unwrap();
        assert_eq!(get_response_after.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_object_not_found() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request =
            common::request_with_auth("DELETE", "/test-bucket/nonexistent.txt", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_object_bucket_not_found() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request =
            common::request_with_auth("DELETE", "/nonexistent-bucket/test-file.txt", Body::empty());

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

        let delete_request_1 =
            common::request_with_auth("DELETE", "/test-bucket/nonexistent.txt", Body::empty());

        let response_1 = app.clone().oneshot(delete_request_1).await.unwrap();
        assert_eq!(response_1.status(), StatusCode::NOT_FOUND);

        let delete_request_2 =
            common::request_with_auth("DELETE", "/test-bucket/nonexistent.txt", Body::empty());

        let response_2 = app.oneshot(delete_request_2).await.unwrap();
        assert_eq!(response_2.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_delete_object_invalid_bucket_name() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request =
            common::request_with_auth("DELETE", "/../../etc/passwd/test-file.txt", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_delete_object_invalid_key() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth(
            "DELETE",
            "/test-bucket/path/../../../etc/passwd",
            Body::empty(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_delete_multiple_objects() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        for i in 1..=3 {
            let uri = format!("/test-bucket/file-{}.txt", i);
            let body = format!("Content {}", i).into_bytes();
            let put_request = common::request_with_auth_and_body("PUT", &uri, body);

            let response = app.clone().oneshot(put_request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }

        for i in 1..=3 {
            let uri = format!("/test-bucket/file-{}.txt", i);
            let delete_request = common::request_with_auth("DELETE", &uri, Body::empty());

            let response = app.clone().oneshot(delete_request).await.unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }

        for i in 1..=3 {
            let uri = format!("/test-bucket/file-{}.txt", i);
            let get_request = common::request_with_auth("GET", &uri, Body::empty());

            let response = app.clone().oneshot(get_request).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }

    #[tokio::test]
    async fn test_delete_ordering_ensures_no_phantom_objects() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state.clone());

        let content = b"Test content for ordering verification";
        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/ordering-test.txt",
            content.to_vec(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let delete_request =
            common::request_with_auth("DELETE", "/test-bucket/ordering-test.txt", Body::empty());

        let delete_response = app.clone().oneshot(delete_request).await.unwrap();
        assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);

        let metadata_result = state
            .metadata
            .get_object_metadata("test-bucket", "ordering-test.txt")
            .await;
        assert!(
            metadata_result.is_err(),
            "Metadata should be deleted (no phantom object)"
        );

        let get_request =
            common::request_with_auth("GET", "/test-bucket/ordering-test.txt", Body::empty());
        let get_response = app.oneshot(get_request).await.unwrap();
        assert_eq!(
            get_response.status(),
            StatusCode::NOT_FOUND,
            "Object should not be retrievable after deletion"
        );
    }

    #[tokio::test]
    async fn test_concurrent_delete_same_object() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let content = b"Concurrent delete test content";
        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/concurrent-delete.txt",
            content.to_vec(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let delete_request1 = common::request_with_auth(
            "DELETE",
            "/test-bucket/concurrent-delete.txt",
            Body::empty(),
        );
        let delete_request2 = common::request_with_auth(
            "DELETE",
            "/test-bucket/concurrent-delete.txt",
            Body::empty(),
        );

        let (result1, result2) = tokio::join!(
            app.clone().oneshot(delete_request1),
            app.clone().oneshot(delete_request2)
        );

        let response1 = result1.unwrap();
        let response2 = result2.unwrap();

        assert!(
            (response1.status() == StatusCode::NO_CONTENT
                && response2.status() == StatusCode::NOT_FOUND)
                || (response1.status() == StatusCode::NOT_FOUND
                    && response2.status() == StatusCode::NO_CONTENT),
            "One DELETE should succeed (204), one should fail (404)"
        );

        let get_request =
            common::request_with_auth("GET", "/test-bucket/concurrent-delete.txt", Body::empty());
        let get_response = app.oneshot(get_request).await.unwrap();
        assert_eq!(
            get_response.status(),
            StatusCode::NOT_FOUND,
            "Object should be fully deleted"
        );
    }
}

mod get {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        common::setup().await
    }

    #[tokio::test]
    async fn test_get_object_success() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let content = b"Hello, World!";

        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/test-file.txt",
            content.to_vec(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_request =
            common::request_with_auth("GET", "/test-bucket/test-file.txt", Body::empty());

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

        let request =
            common::request_with_auth("GET", "/test-bucket/nonexistent.txt", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_object_bucket_not_found() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request =
            common::request_with_auth("GET", "/nonexistent-bucket/test-file.txt", Body::empty());

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

        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/etag-test.txt",
            content.to_vec(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let put_etag = put_response
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap();

        let get_request =
            common::request_with_auth("GET", "/test-bucket/etag-test.txt", Body::empty());

        let get_response = app.oneshot(get_request).await.unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);

        let get_etag = get_response
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap();

        assert_eq!(get_etag, put_etag);
    }

    #[tokio::test]
    async fn test_get_object_large_file() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let content = vec![0xAB; 100_000];

        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/large-file.bin",
            content.clone(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let get_request =
            common::request_with_auth("GET", "/test-bucket/large-file.bin", Body::empty());

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

        let request =
            common::request_with_auth("GET", "/../../etc/passwd/test-file.txt", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_get_object_invalid_key() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth(
            "GET",
            "/test-bucket/path/../../../etc/passwd",
            Body::empty(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

mod head {
    use super::*;
    use axum::http::Request;

    async fn setup() -> (save_api::AppState, TempDir) {
        common::setup().await
    }

    #[tokio::test]
    async fn test_head_object_success() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let content = b"Hello, World!";

        // Create an object
        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/test-file.txt",
            content.to_vec(),
        );

        let put_response = app.clone().oneshot(put_request).await.unwrap();
        assert_eq!(put_response.status(), StatusCode::OK);

        let head_request =
            common::request_with_auth("HEAD", "/test-bucket/test-file.txt", Body::empty());

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

        let request =
            common::request_with_auth("HEAD", "/test-bucket/nonexistent.txt", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_head_object_bucket_not_found() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request =
            common::request_with_auth("HEAD", "/nonexistent-bucket/test-file.txt", Body::empty());

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
        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/header-test.txt",
            content.to_vec(),
        );

        app.clone().oneshot(put_request).await.unwrap();

        let head_request =
            common::request_with_auth("HEAD", "/test-bucket/header-test.txt", Body::empty());

        let head_response = app.clone().oneshot(head_request).await.unwrap();

        let get_request =
            common::request_with_auth("GET", "/test-bucket/header-test.txt", Body::empty());

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

        let request =
            common::request_with_auth("HEAD", "/../../etc/passwd/test-file.txt", Body::empty());

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_head_object_invalid_key() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth(
            "HEAD",
            "/test-bucket/path/../../../etc/passwd",
            Body::empty(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_head_object_large_file() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let content = vec![0xAB; 100_000];

        // Create a large object
        let put_request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/large-file.bin",
            content.to_vec(),
        );

        app.clone().oneshot(put_request).await.unwrap();

        let head_request =
            common::request_with_auth("HEAD", "/test-bucket/large-file.bin", Body::empty());

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
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListBucketResult");
        assert!(body_str.contains("xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\""));

        assert_eq!(get_element_text(root, "Name"), Some("test-bucket"));
        assert_eq!(get_element_text(root, "IsTruncated"), Some("false"));

        let contents = get_all_elements(root, "Contents");
        assert_eq!(contents.len(), 3);

        let mut keys: Vec<_> = contents
            .iter()
            .filter_map(|c| get_element_text(*c, "Key"))
            .collect();
        keys.sort();

        let mut expected = vec!["file1.txt", "file2.txt", "dir/file3.txt"];
        expected.sort();
        assert_eq!(keys, expected);

        for content in &contents {
            assert!(get_element_text(*content, "Size").is_some());
            assert!(get_element_text(*content, "ETag").is_some());
            assert!(get_element_text(*content, "LastModified").is_some());
            assert_eq!(get_element_text(*content, "StorageClass"), Some("STANDARD"));
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
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListBucketResult");
        assert!(body_str.contains("xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\""));

        assert_eq!(get_element_text(root, "Name"), Some("test-bucket"));

        let contents = get_all_elements(root, "Contents");
        assert_eq!(contents.len(), 0);
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
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListBucketResult");
        assert_eq!(get_element_text(root, "Prefix"), Some("dir/"));

        let contents = get_all_elements(root, "Contents");
        assert_eq!(contents.len(), 2);

        let keys: Vec<_> = contents
            .iter()
            .filter_map(|c| get_element_text(*c, "Key"))
            .collect();

        assert_eq!(keys, vec!["dir/file2.txt", "dir/file3.txt"]);
        assert!(!keys.contains(&"file1.txt"));
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
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListBucketResult");

        assert_eq!(get_element_text(root, "Marker"), Some("a-file.txt"));

        // Validate only objects after marker are included
        let contents = get_all_elements(root, "Contents");
        assert_eq!(contents.len(), 2);

        let keys: Vec<_> = contents
            .iter()
            .filter_map(|c| get_element_text(*c, "Key"))
            .collect();

        assert_eq!(keys, vec!["b-file.txt", "c-file.txt"]);
        assert!(!keys.contains(&"a-file.txt"));
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
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        let doc = parse_xml(&body_str);
        let root = doc.root_element();

        assert_eq!(root.tag_name().name(), "ListBucketResult");

        assert_eq!(get_element_text(root, "MaxKeys"), Some("5"));
        assert_eq!(get_element_text(root, "IsTruncated"), Some("true"));

        let contents = get_all_elements(root, "Contents");
        assert_eq!(contents.len(), 5);
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
        common::setup().await
    }

    #[tokio::test]
    async fn test_put_object_success() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/test-file.txt",
            b"Hello, World!".to_vec(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // S3 PutObject returns ETag in header, not body
        let etag = response.headers().get("etag").unwrap().to_str().unwrap();
        assert!(!etag.is_empty());
        assert!(etag.starts_with('"') && etag.ends_with('"'));
    }

    #[tokio::test]
    async fn test_put_object_bucket_not_found() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth_and_body(
            "PUT",
            "/nonexistent-bucket/test-file.txt",
            b"Hello, World!".to_vec(),
        );

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

        // Test with invalid auth format (not AWS4-HMAC-SHA256)
        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/test-file.txt")
            .header("Authorization", "Basic invalid-auth-format")
            .header("x-amz-date", "20240101T000000Z")
            .header(
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            )
            .body(Body::from(b"Hello, World!".to_vec()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_put_object_wrong_access_key() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        // Use valid signature but wrong access key
        let (authorization, amz_date, payload_hash) = common::sign_request(
            "PUT",
            "/test-bucket/test-file.txt",
            b"Hello, World!",
            "wrong-key",
            "test-access-key",
        );

        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/test-file.txt")
            .header("Authorization", authorization)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .header("host", "localhost:9000")
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

        let request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/etag-test.txt",
            content.to_vec(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // S3 PutObject returns ETag in header, not body
        let etag_header = response.headers().get("etag").unwrap().to_str().unwrap();
        let etag = etag_header.trim_matches('"');

        assert_eq!(etag, expected_etag);
    }

    #[tokio::test]
    async fn test_put_object_invalid_bucket_name() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth_and_body(
            "PUT",
            "/../../etc/passwd/test-file.txt",
            b"Hello, World!".to_vec(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_put_object_invalid_key() {
        let (state, _temp_dir) = setup().await;
        let app = save_api::app(state);

        let request = common::request_with_auth_and_body(
            "PUT",
            "/test-bucket/path/../../../etc/passwd",
            b"Hello, World!".to_vec(),
        );

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
