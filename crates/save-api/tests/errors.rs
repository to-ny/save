mod common;

use axum::{body::Body, http::StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn assert_xml_contains(xml: &str, tag: &str, content: &str) {
    let expected = format!("<{tag}>{content}</{tag}>");
    assert!(
        xml.contains(&expected),
        "Expected XML to contain '{}' but got: {}",
        expected,
        xml
    );
}

fn assert_xml_has_tag(xml: &str, tag: &str) {
    assert!(
        xml.contains(&format!("<{tag}>")) && xml.contains(&format!("</{tag}>")),
        "Expected XML to have tag '{}' but got: {}",
        tag,
        xml
    );
}

async fn response_to_string(response: axum::http::Response<Body>) -> String {
    let body = response.into_body();
    let bytes = body.collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn test_bucket_not_found_error() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("GET", "/nonexistent-bucket", Body::empty());
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/xml"
    );

    let body = response_to_string(response).await;
    assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert_xml_contains(&body, "Code", "NoSuchBucket");
    assert_xml_contains(&body, "Message", "The specified bucket does not exist");
    assert_xml_contains(&body, "Resource", "/nonexistent-bucket");
    assert_xml_has_tag(&body, "RequestId");
}

#[tokio::test]
async fn test_bucket_already_exists_error() {
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
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/xml"
    );

    let body = response_to_string(response).await;
    assert_xml_contains(&body, "Code", "BucketAlreadyExists");
    assert!(body.contains("already exists"));
    assert_xml_contains(&body, "Resource", "/existing-bucket");
}

#[tokio::test]
async fn test_bucket_not_empty_error() {
    let (state, _temp_dir) = common::setup().await;

    let object_metadata = save_metadata::ObjectMetadata::new(
        "test-bucket".to_string(),
        "test-key".to_string(),
        100,
        "etag123".to_string(),
    );
    state
        .metadata
        .put_object_metadata(object_metadata)
        .await
        .unwrap();

    let app = save_api::app(state);
    let request = common::request_with_auth("DELETE", "/test-bucket", Body::empty());
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body = response_to_string(response).await;
    assert_xml_contains(&body, "Code", "BucketNotEmpty");
    assert!(body.contains("not empty"));
}

#[tokio::test]
async fn test_object_not_found_error() {
    let (state, _temp_dir) = common::setup().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("GET", "/test-bucket/nonexistent.txt", Body::empty());
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = response_to_string(response).await;
    assert_xml_contains(&body, "Code", "NoSuchKey");
    assert_xml_contains(&body, "Message", "The specified key does not exist");
    assert_xml_contains(&body, "Resource", "/test-bucket/nonexistent.txt");
}

#[tokio::test]
async fn test_invalid_bucket_name_error() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("PUT", "/ab", Body::empty());
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/xml"
    );

    let body = response_to_string(response).await;
    assert_xml_contains(&body, "Code", "InvalidRequest");
    assert!(body.contains("Bucket") || body.contains("bucket") || body.contains("name"));
}

#[tokio::test]
async fn test_unauthorized_error() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = axum::http::Request::builder()
        .method("GET")
        .uri("/test-bucket")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/xml"
    );

    let body = response_to_string(response).await;
    assert_xml_contains(&body, "Code", "AccessDenied");
}

#[tokio::test]
async fn test_error_response_has_request_id() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("GET", "/nonexistent", Body::empty());
    let response = app.oneshot(request).await.unwrap();

    let body = response_to_string(response).await;
    assert_xml_has_tag(&body, "RequestId");

    let start = body.find("<RequestId>").unwrap() + "<RequestId>".len();
    let end = body.find("</RequestId>").unwrap();
    let request_id = &body[start..end];

    assert!(!request_id.is_empty(), "RequestId should not be empty");
}

#[tokio::test]
async fn test_multiple_errors_have_unique_request_ids() {
    let (state, _temp_dir) = common::setup_empty().await;

    let app1 = save_api::app(state.clone());
    let app2 = save_api::app(state);

    let request1 = common::request_with_auth("GET", "/bucket1", Body::empty());
    let request2 = common::request_with_auth("GET", "/bucket2", Body::empty());

    let response1 = app1.oneshot(request1).await.unwrap();
    let response2 = app2.oneshot(request2).await.unwrap();

    let body1 = response_to_string(response1).await;
    let body2 = response_to_string(response2).await;

    let extract_request_id = |body: &str| -> String {
        let start = body.find("<RequestId>").unwrap() + "<RequestId>".len();
        let end = body.find("</RequestId>").unwrap();
        body[start..end].to_string()
    };

    let id1 = extract_request_id(&body1);
    let id2 = extract_request_id(&body2);

    assert_ne!(id1, id2, "Request IDs should be unique");
}

#[tokio::test]
async fn test_xml_wellformedness() {
    let (state, _temp_dir) = common::setup_empty().await;
    let app = save_api::app(state);

    let request = common::request_with_auth("GET", "/test", Body::empty());
    let response = app.oneshot(request).await.unwrap();

    let body = response_to_string(response).await;

    assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(body.contains("<Error>"));
    assert!(body.contains("</Error>"));

    let error_count = body.matches("<Error>").count();
    assert_eq!(error_count, body.matches("</Error>").count());
}

#[tokio::test]
async fn test_concurrent_request_ids_are_unique() {
    use std::collections::HashSet;
    use tokio::task::JoinSet;

    let (state, _temp_dir) = common::setup_empty().await;

    let mut set = JoinSet::new();
    for _ in 0..100 {
        let state_clone = state.clone();
        set.spawn(async move {
            let app = save_api::app(state_clone);
            let request = common::request_with_auth("GET", "/nonexistent-bucket", Body::empty());
            let response = app.oneshot(request).await.unwrap();
            let body = response_to_string(response).await;

            let start = body.find("<RequestId>").unwrap() + "<RequestId>".len();
            let end = body.find("</RequestId>").unwrap();
            body[start..end].to_string()
        });
    }

    let mut ids = HashSet::new();
    while let Some(Ok(id)) = set.join_next().await {
        assert!(ids.insert(id.clone()), "Duplicate request ID found: {}", id);
    }

    assert_eq!(ids.len(), 100, "Should have 100 unique request IDs");
}

#[tokio::test]
async fn test_error_xml_is_wellformed_and_escaped() {
    use save_common::S3Error;

    let error = S3Error::invalid_request("Bucket name contains invalid chars: <test> & \"quotes\"");
    let xml = error.to_xml();

    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));

    assert!(!xml.contains("<test>"), "< and > should be escaped");
    assert!(
        xml.contains("&lt;test&gt;") || xml.contains("&amp;"),
        "Special chars should be escaped"
    );

    assert!(
        !xml.contains("& \"quotes\"") || xml.contains("&amp;") || xml.contains("&quot;"),
        "Ampersands and quotes should be escaped"
    );
}
