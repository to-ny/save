mod common;

use axum::{body::Body, http::StatusCode};
use chrono::{Duration, Utc};
use common::{parse_xml, sign_request};
use http_body_util::BodyExt;
use save_common::config::CredentialsConfig;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

#[tokio::test]
async fn test_expired_timestamp_rejected() {
    let (state, _temp_dir) = common::setup().await;
    let app = save_api::app(state);

    let old_time = Utc::now() - Duration::seconds(16 * 60);
    let expired_date = old_time.format("%Y%m%dT%H%M%SZ").to_string();

    let body = b"test content";
    let mut hasher = Sha256::new();
    hasher.update(body);
    let payload_hash = hex::encode(hasher.finalize());

    let (authorization, _, _) = sign_request(
        "PUT",
        "/test-bucket/test-file.txt",
        body,
        "saveadmin",
        "savepass",
    );

    let request = axum::http::Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header("Authorization", authorization)
        .header("x-amz-date", expired_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("host", "localhost:9000")
        .body(Body::from(body.to_vec()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();

    let doc = parse_xml(&body_str);
    let code = common::get_element_text(doc.root(), "Code").unwrap();
    assert_eq!(code, "RequestTimeTooSkewed");
}

#[tokio::test]
async fn test_future_timestamp_rejected() {
    let (state, _temp_dir) = common::setup().await;
    let app = save_api::app(state);

    let future_time = Utc::now() + Duration::seconds(16 * 60);
    let future_date = future_time.format("%Y%m%dT%H%M%SZ").to_string();

    let body = b"test content";
    let mut hasher = Sha256::new();
    hasher.update(body);
    let payload_hash = hex::encode(hasher.finalize());

    let (authorization, _, _) = sign_request(
        "PUT",
        "/test-bucket/test-file.txt",
        body,
        "saveadmin",
        "savepass",
    );

    let request = axum::http::Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header("Authorization", authorization)
        .header("x-amz-date", future_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("host", "localhost:9000")
        .body(Body::from(body.to_vec()))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();

    let doc = parse_xml(&body_str);
    let code = common::get_element_text(doc.root(), "Code").unwrap();
    assert_eq!(code, "RequestTimeTooSkewed");
}

#[tokio::test]
async fn test_missing_authorization_header() {
    let (state, _temp_dir) = common::setup().await;
    let app = save_api::app(state);

    let request = axum::http::Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .body(Body::from("test"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_invalid_authorization_format() {
    let (state, _temp_dir) = common::setup().await;
    let app = save_api::app(state);

    let request = axum::http::Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header("Authorization", "Basic dXNlcjpwYXNz")
        .header(
            "x-amz-date",
            Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        )
        .body(Body::from("test"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_missing_amz_date_header() {
    let (state, _temp_dir) = common::setup().await;
    let app = save_api::app(state);

    let request = axum::http::Request::builder()
        .method("PUT")
        .uri("/test-bucket/test-file.txt")
        .header("Authorization", "AWS4-HMAC-SHA256 Credential=saveadmin/20240101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=fake")
        .header("host", "localhost:9000")
        .body(Body::from("test"))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn test_credentials_debug_redacted() {
    let creds = CredentialsConfig {
        access_key: "my-access-key".to_string(),
        secret_key: "my-secret-key".to_string(),
    };

    let debug_output = format!("{:?}", creds);

    assert!(
        !debug_output.contains("my-access-key"),
        "Debug output contains access key: {}",
        debug_output
    );
    assert!(
        !debug_output.contains("my-secret-key"),
        "Debug output contains secret key: {}",
        debug_output
    );
    assert!(
        debug_output.contains("[REDACTED]"),
        "Debug output should contain [REDACTED]: {}",
        debug_output
    );
}
