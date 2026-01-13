//! Request forwarding to Raft leader for non-leader nodes.

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use http_body_util::BodyExt;
use reqwest::Client;
use save_common::tracing::{PARENT_SPAN_HEADER, REQUEST_ID_HEADER, TRACE_ID_HEADER};
use std::time::Duration;
use tracing::{debug, warn};

/// HTTP client for forwarding requests to the Raft leader.
#[derive(Clone)]
pub struct ForwardingClient {
    client: Client,
}

impl ForwardingClient {
    pub fn new(timeout: Duration, connect_timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .connect_timeout(connect_timeout)
            .pool_max_idle_per_host(10)
            .build()
            .expect("failed to build forwarding client");

        Self { client }
    }

    /// Forwards an HTTP request to the given leader address.
    pub async fn forward(
        &self,
        leader_addr: &str,
        request: Request,
    ) -> Result<Response, ForwardError> {
        let (parts, body) = request.into_parts();

        let body_bytes = body
            .collect()
            .await
            .map_err(|e| ForwardError::BodyRead(e.to_string()))?
            .to_bytes();

        let target_uri = build_target_uri(leader_addr, &parts.uri)?;

        debug!(
            method = %parts.method,
            uri = %target_uri,
            "Forwarding request to leader"
        );

        let mut req_builder = self
            .client
            .request(parts.method.clone(), target_uri.to_string())
            .body(body_bytes.to_vec());

        // Forward headers needed for SigV4 signature verification
        for (name, value) in parts.headers.iter() {
            if let Ok(v) = value.to_str()
                && should_forward_header(name.as_str())
            {
                req_builder = req_builder.header(name.as_str(), v);
            }
        }

        // Add forwarding marker to prevent loops
        req_builder = req_builder.header("x-forwarded-by", "save-node");

        let response = req_builder
            .send()
            .await
            .map_err(|e| ForwardError::Request(e.to_string()))?;

        // Convert reqwest response to axum response
        let status = StatusCode::from_u16(response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let headers = response.headers().clone();
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| ForwardError::ResponseRead(e.to_string()))?;

        let mut response = Response::new(Body::from(body_bytes));
        *response.status_mut() = status;

        // Copy response headers
        for (name, value) in headers.iter() {
            if let Ok(hv) = HeaderValue::from_bytes(value.as_bytes()) {
                response.headers_mut().insert(name.clone(), hv);
            }
        }

        // Mark as forwarded
        response.headers_mut().insert(
            "x-forwarded-from",
            HeaderValue::from_str(leader_addr).unwrap_or(HeaderValue::from_static("unknown")),
        );

        Ok(response)
    }
}

fn build_target_uri(leader_addr: &str, original_uri: &Uri) -> Result<Uri, ForwardError> {
    let path_and_query = original_uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let target = format!("{}{}", leader_addr, path_and_query);
    target
        .parse::<Uri>()
        .map_err(|e| ForwardError::InvalidUri(e.to_string()))
}

fn should_forward_header(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "content-type"
            | "content-length"
            | "authorization"
            | "x-amz-date"
            | "x-amz-content-sha256"
            | "x-amz-decoded-content-length"
            | "x-amz-meta-"
            | "host"
    ) || lower.starts_with("x-amz-")
        || lower == TRACE_ID_HEADER
        || lower == PARENT_SPAN_HEADER
        || lower == REQUEST_ID_HEADER
}

#[derive(Debug)]
pub enum ForwardError {
    BodyRead(String),
    Request(String),
    ResponseRead(String),
    InvalidUri(String),
}

impl std::fmt::Display for ForwardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForwardError::BodyRead(e) => write!(f, "failed to read request body: {}", e),
            ForwardError::Request(e) => write!(f, "forward request failed: {}", e),
            ForwardError::ResponseRead(e) => write!(f, "failed to read response body: {}", e),
            ForwardError::InvalidUri(e) => write!(f, "invalid target URI: {}", e),
        }
    }
}

impl std::error::Error for ForwardError {}

impl IntoResponse for ForwardError {
    fn into_response(self) -> Response {
        warn!(error = %self, "Request forwarding failed");

        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Error>
    <Code>ServiceUnavailable</Code>
    <Message>{}</Message>
</Error>"#,
            self
        );

        (
            StatusCode::BAD_GATEWAY,
            [("content-type", "application/xml")],
            body,
        )
            .into_response()
    }
}

/// Checks if a request has already been forwarded (to prevent loops).
pub fn is_already_forwarded(headers: &HeaderMap) -> bool {
    headers.contains_key("x-forwarded-by")
}

/// Returns true if the given HTTP method requires write (leader) access.
pub fn is_write_method(method: &axum::http::Method) -> bool {
    matches!(
        *method,
        axum::http::Method::PUT
            | axum::http::Method::POST
            | axum::http::Method::DELETE
            | axum::http::Method::PATCH
    )
}

/// Returns true if the given HTTP method is a read operation.
pub fn is_read_method(method: &axum::http::Method) -> bool {
    matches!(*method, axum::http::Method::GET | axum::http::Method::HEAD)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;

    #[test]
    fn test_should_forward_header() {
        // SigV4-relevant headers must be forwarded
        assert!(should_forward_header("content-type"));
        assert!(should_forward_header("Content-Type"));
        assert!(should_forward_header("authorization"));
        assert!(should_forward_header("x-amz-date"));
        assert!(should_forward_header("x-amz-content-sha256"));
        assert!(should_forward_header("x-amz-meta-custom"));
        assert!(should_forward_header(TRACE_ID_HEADER));
        assert!(should_forward_header(REQUEST_ID_HEADER));

        // Host header is critical for SigV4 signature verification
        assert!(
            should_forward_header("host"),
            "Host header must be forwarded for SigV4 signatures"
        );
        assert!(should_forward_header("Host"));

        // Hop-by-hop headers should not be forwarded
        assert!(!should_forward_header("connection"));
        assert!(!should_forward_header("accept-encoding"));
        assert!(!should_forward_header("transfer-encoding"));
    }

    #[test]
    fn test_build_target_uri() {
        let original: Uri = "/bucket/key?uploads".parse().unwrap();
        let result = build_target_uri("http://192.168.1.10:9000", &original).unwrap();
        assert_eq!(
            result.to_string(),
            "http://192.168.1.10:9000/bucket/key?uploads"
        );
    }

    #[test]
    fn test_build_target_uri_root() {
        let original: Uri = "/".parse().unwrap();
        let result = build_target_uri("http://leader:9000", &original).unwrap();
        assert_eq!(result.to_string(), "http://leader:9000/");
    }

    #[test]
    fn test_is_already_forwarded() {
        let mut headers = HeaderMap::new();
        assert!(!is_already_forwarded(&headers));

        headers.insert("x-forwarded-by", HeaderValue::from_static("save-node"));
        assert!(is_already_forwarded(&headers));
    }

    #[test]
    fn test_is_write_method() {
        use axum::http::Method;

        assert!(is_write_method(&Method::PUT));
        assert!(is_write_method(&Method::POST));
        assert!(is_write_method(&Method::DELETE));
        assert!(is_write_method(&Method::PATCH));

        assert!(!is_write_method(&Method::GET));
        assert!(!is_write_method(&Method::HEAD));
        assert!(!is_write_method(&Method::OPTIONS));
    }

    #[test]
    fn test_is_read_method() {
        use axum::http::Method;

        assert!(is_read_method(&Method::GET));
        assert!(is_read_method(&Method::HEAD));

        assert!(!is_read_method(&Method::PUT));
        assert!(!is_read_method(&Method::POST));
        assert!(!is_read_method(&Method::DELETE));
        assert!(!is_read_method(&Method::OPTIONS));
    }

    #[test]
    fn test_build_target_uri_with_complex_query() {
        let original: Uri = "/bucket/key?uploadId=abc123&partNumber=1".parse().unwrap();
        let result = build_target_uri("http://leader:9000", &original).unwrap();
        assert_eq!(
            result.to_string(),
            "http://leader:9000/bucket/key?uploadId=abc123&partNumber=1"
        );
    }

    #[test]
    fn test_build_target_uri_with_encoded_path() {
        let original: Uri = "/bucket/path%20with%20spaces/key".parse().unwrap();
        let result = build_target_uri("http://leader:9000", &original).unwrap();
        assert!(result.to_string().contains("path%20with%20spaces"));
    }

    #[test]
    fn test_build_target_uri_invalid() {
        let original: Uri = "/bucket/key".parse().unwrap();
        // Invalid leader address with spaces
        let result = build_target_uri("http://leader with spaces:9000", &original);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ForwardError::InvalidUri(_)));
    }

    #[test]
    fn test_forward_error_display() {
        let err = ForwardError::BodyRead("connection reset".to_string());
        assert_eq!(
            err.to_string(),
            "failed to read request body: connection reset"
        );

        let err = ForwardError::Request("timeout".to_string());
        assert_eq!(err.to_string(), "forward request failed: timeout");

        let err = ForwardError::ResponseRead("incomplete".to_string());
        assert_eq!(err.to_string(), "failed to read response body: incomplete");

        let err = ForwardError::InvalidUri("bad uri".to_string());
        assert_eq!(err.to_string(), "invalid target URI: bad uri");
    }

    #[tokio::test]
    async fn test_forward_error_into_response() {
        let err = ForwardError::Request("connection refused".to_string());
        let response = err.into_response();

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/xml"
        );

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("<Code>ServiceUnavailable</Code>"));
        assert!(body_str.contains("connection refused"));
    }

    #[tokio::test]
    async fn test_forwarding_client_to_unreachable_host() {
        let client = ForwardingClient::new(Duration::from_millis(100), Duration::from_millis(50));

        let request = Request::builder()
            .method("PUT")
            .uri("/test-bucket/test-key")
            .body(Body::from("test data"))
            .unwrap();

        // Try to forward to a non-existent host
        let result = client.forward("http://192.0.2.1:9999", request).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ForwardError::Request(_)));
    }

    #[tokio::test]
    async fn test_forwarding_client_invalid_uri() {
        let client = ForwardingClient::new(Duration::from_secs(5), Duration::from_secs(2));

        let request = Request::builder()
            .method("PUT")
            .uri("/bucket/key")
            .body(Body::empty())
            .unwrap();

        // Invalid URI with control characters
        let result = client.forward("http://\x00invalid", request).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ForwardError::InvalidUri(_)));
    }

    #[test]
    fn test_should_forward_header_tracing_headers() {
        // Verify all tracing headers are forwarded
        assert!(should_forward_header(TRACE_ID_HEADER));
        assert!(should_forward_header(PARENT_SPAN_HEADER));
        assert!(should_forward_header(REQUEST_ID_HEADER));
    }

    #[test]
    fn test_should_forward_header_all_amz_headers() {
        // Any x-amz-* header should be forwarded
        assert!(should_forward_header("x-amz-copy-source"));
        assert!(should_forward_header("x-amz-storage-class"));
        assert!(should_forward_header("x-amz-server-side-encryption"));
        assert!(should_forward_header("X-Amz-Acl"));
    }

    #[test]
    fn test_should_not_forward_internal_headers() {
        assert!(!should_forward_header("x-forwarded-by"));
        assert!(!should_forward_header("x-forwarded-for"));
        assert!(!should_forward_header("x-forwarded-proto"));
    }

    #[test]
    fn test_is_already_forwarded_case_insensitive() {
        let mut headers = HeaderMap::new();
        // Header names are case-insensitive in HTTP
        headers.insert("X-Forwarded-By", HeaderValue::from_static("other-node"));
        assert!(is_already_forwarded(&headers));
    }
}
