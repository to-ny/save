use axum::{extract::Request, middleware::Next, response::Response};
use save_common::tracing::{
    PARENT_SPAN_HEADER, REQUEST_ID_HEADER, TRACE_ID_HEADER, TraceContext, generate_span_id,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tracing::{Span, debug, warn};
use uuid::Uuid;

use crate::forward::{is_already_forwarded, is_write_method};
use crate::metrics::{http_request_duration_seconds, http_requests_total};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use save_common::s3_error::S3Error;

/// Create a trace context from incoming HTTP headers.
fn trace_context_from_headers(headers: &axum::http::HeaderMap) -> TraceContext {
    let trace_id = headers
        .get(TRACE_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let parent_span_id = headers
        .get(PARENT_SPAN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let request_id = headers
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    TraceContext::with_values(trace_id, generate_span_id(), parent_span_id, request_id)
}

#[derive(Clone)]
pub struct RequestTracker {
    in_flight: Arc<AtomicUsize>,
}

impl RequestTracker {
    pub fn new() -> Self {
        Self {
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn in_flight_count(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    fn increment(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
    }

    fn decrement(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Default for RequestTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn track_requests(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    let should_track = !matches!(path, "/metrics" | "/health" | "/health/ready");

    if should_track {
        state.request_tracker.increment();
        crate::metrics::in_flight_requests().inc();
    }

    let response = next.run(request).await;

    if should_track {
        state.request_tracker.decrement();
        crate::metrics::in_flight_requests().dec();
    }
    response
}

pub async fn request_id(mut request: Request, next: Next) -> Response {
    let trace_ctx = trace_context_from_headers(request.headers());

    // Record trace context in current span
    Span::current().record("trace_id", &trace_ctx.trace_id);
    Span::current().record("span_id", &trace_ctx.span_id);
    Span::current().record("request_id", &trace_ctx.request_id);
    if let Some(ref parent) = trace_ctx.parent_span_id {
        Span::current().record("parent_span_id", parent);
    }

    // Store in request extensions for use by handlers
    request.extensions_mut().insert(trace_ctx.clone());

    let mut response = next.run(request).await;

    // Add trace headers to response
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        trace_ctx
            .request_id
            .parse()
            .expect("request_id is valid header value"),
    );
    response.headers_mut().insert(
        TRACE_ID_HEADER,
        trace_ctx
            .trace_id
            .parse()
            .expect("trace_id is valid header value"),
    );
    response.headers_mut().insert(
        PARENT_SPAN_HEADER,
        trace_ctx
            .span_id
            .parse()
            .expect("span_id is valid header value"),
    );

    response
}

pub async fn track_metrics(request: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().to_string();
    let endpoint = normalize_endpoint(request.uri().path());

    let trace_ctx = request
        .extensions()
        .get::<TraceContext>()
        .cloned()
        .unwrap_or_default();

    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    http_requests_total()
        .with_label_values(&[endpoint, &method, &status])
        .inc();

    http_request_duration_seconds()
        .with_label_values(&[endpoint, &method])
        .observe(duration);

    debug!(
        trace_id = %trace_ctx.trace_id,
        span_id = %trace_ctx.span_id,
        request_id = %trace_ctx.request_id,
        endpoint = %endpoint,
        method = %method,
        status = %status,
        duration_seconds = %duration,
        "Request completed"
    );

    response
}

fn normalize_endpoint(path: &str) -> &'static str {
    match path {
        "/health" => "/health",
        "/metrics" => "/metrics",
        "/" => "/",
        _ => {
            let slash_count = path.bytes().filter(|&b| b == b'/').count();
            if slash_count == 1 {
                "/{bucket}"
            } else {
                "/{bucket}/{key}"
            }
        }
    }
}

/// Forwards write requests to the Raft leader if this node is not the leader.
pub async fn forward_to_leader(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // Skip forwarding for health/metrics endpoints
    if matches!(
        path,
        "/metrics" | "/health" | "/health/ready" | "/cluster/status"
    ) {
        return next.run(request).await;
    }

    // Only forward write operations
    if !is_write_method(request.method()) {
        return next.run(request).await;
    }

    // Prevent forwarding loops
    if is_already_forwarded(request.headers()) {
        warn!("Request already forwarded, processing locally");
        return next.run(request).await;
    }

    // Check if we're the leader
    if state.raft_node.is_leader().await {
        return next.run(request).await;
    }

    // Not the leader - forward to leader using their HTTP API address
    let leader_addr = match state.raft_node.leader_http_addr().await {
        Some(addr) => addr,
        None => {
            debug!("No leader available, attempting local processing");
            return next.run(request).await;
        }
    };

    let method = request.method().to_string();
    let start = Instant::now();

    debug!(leader_addr = %leader_addr, "Forwarding request to leader");

    match state.forwarding_client.forward(&leader_addr, request).await {
        Ok(response) => {
            crate::metrics::record_forwarded_request(true);
            crate::metrics::record_forwarding_latency(&method, start.elapsed().as_secs_f64());
            response
        }
        Err(e) => {
            crate::metrics::record_forwarded_request(false);
            crate::metrics::record_forwarding_latency(&method, start.elapsed().as_secs_f64());
            warn!(error = %e, "Failed to forward request to leader");
            e.into_response()
        }
    }
}

/// Paths exempt from the cluster initialization check.
const EXEMPT_PATHS: &[&str] = &[
    "/cluster/initialize",
    "/cluster/status",
    "/cluster/members",
    "/cluster/members/promote",
    "/health",
    "/health/ready",
    "/metrics",
];

/// Check if a path is exempt from cluster initialization check.
fn is_exempt_from_initialization_check(path: &str) -> bool {
    // Exact matches
    if EXEMPT_PATHS.contains(&path) {
        return true;
    }
    // Pattern match for /cluster/members/{node_id} (DELETE endpoint)
    if path.starts_with("/cluster/members/") && path != "/cluster/members/promote" {
        return true;
    }
    false
}

/// Rejects client requests when the Raft cluster is not initialized.
/// Returns HTTP 503 Service Unavailable with an S3-compatible XML error response.
pub async fn require_initialized_cluster(
    axum::extract::State(state): axum::extract::State<crate::AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();

    // Allow exempt paths regardless of initialization state
    if is_exempt_from_initialization_check(path) {
        return next.run(request).await;
    }

    // Check if cluster is initialized
    if state.raft_node.is_initialized() {
        return next.run(request).await;
    }

    // Cluster is not initialized - reject the request
    warn!(path = %path, "Rejecting request: cluster is not initialized");

    let error = S3Error::service_unavailable(
        "Cluster is not initialized. Please initialize the cluster first.",
    );

    // Get request ID from extensions if available
    let request_id = request
        .extensions()
        .get::<save_common::tracing::TraceContext>()
        .map(|ctx| ctx.request_id.clone());

    let error = if let Some(req_id) = request_id {
        error.with_request_id(req_id)
    } else {
        error
    };

    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "application/xml"),
            (
                header::HeaderName::from_static("x-amz-request-id"),
                error.request_id.as_str(),
            ),
        ],
        error.to_xml(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_endpoint() {
        assert_eq!(normalize_endpoint("/health"), "/health");
        assert_eq!(normalize_endpoint("/metrics"), "/metrics");
        assert_eq!(normalize_endpoint("/my-bucket"), "/{bucket}");
        assert_eq!(
            normalize_endpoint("/my-bucket/my-object.txt"),
            "/{bucket}/{key}"
        );
        assert_eq!(
            normalize_endpoint("/my-bucket/folder/subfolder/object.txt"),
            "/{bucket}/{key}"
        );
        assert_eq!(normalize_endpoint("/"), "/");
    }

    #[tokio::test]
    async fn test_track_metrics_middleware() {
        use axum::{Router, body::Body, http::StatusCode, routing::get};
        use tower::ServiceExt;

        async fn handler() -> &'static str {
            "OK"
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(axum::middleware::from_fn(track_metrics));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Verify metrics were recorded
        let metrics = crate::metrics::encode_metrics().unwrap();
        assert!(metrics.contains("save_http_requests_total"));
        assert!(metrics.contains("save_http_request_duration_seconds"));
    }

    #[test]
    fn test_trace_context_from_headers_empty() {
        let headers = axum::http::HeaderMap::new();
        let ctx = trace_context_from_headers(&headers);

        // Should generate new IDs when headers are missing
        assert!(!ctx.trace_id.is_empty());
        assert!(!ctx.span_id.is_empty());
        assert!(!ctx.request_id.is_empty());
        assert!(ctx.parent_span_id.is_none());
    }

    #[test]
    fn test_trace_context_from_headers_with_trace() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(TRACE_ID_HEADER, "test-trace-id".parse().unwrap());
        headers.insert(PARENT_SPAN_HEADER, "parent-span-123".parse().unwrap());
        headers.insert(REQUEST_ID_HEADER, "req-456".parse().unwrap());

        let ctx = trace_context_from_headers(&headers);

        assert_eq!(ctx.trace_id, "test-trace-id");
        assert_eq!(ctx.parent_span_id, Some("parent-span-123".to_string()));
        assert_eq!(ctx.request_id, "req-456");
        assert!(!ctx.span_id.is_empty()); // New span ID generated
    }

    #[test]
    fn test_is_exempt_from_initialization_check() {
        // Exempt paths - exact matches
        assert!(is_exempt_from_initialization_check("/cluster/initialize"));
        assert!(is_exempt_from_initialization_check("/cluster/status"));
        assert!(is_exempt_from_initialization_check("/cluster/members"));
        assert!(is_exempt_from_initialization_check(
            "/cluster/members/promote"
        ));
        assert!(is_exempt_from_initialization_check("/health"));
        assert!(is_exempt_from_initialization_check("/health/ready"));
        assert!(is_exempt_from_initialization_check("/metrics"));

        // Exempt paths - pattern match for /cluster/members/{node_id}
        assert!(is_exempt_from_initialization_check("/cluster/members/1"));
        assert!(is_exempt_from_initialization_check(
            "/cluster/members/node-123"
        ));

        // Non-exempt paths - S3 API
        assert!(!is_exempt_from_initialization_check("/"));
        assert!(!is_exempt_from_initialization_check("/my-bucket"));
        assert!(!is_exempt_from_initialization_check("/my-bucket/my-key"));
        assert!(!is_exempt_from_initialization_check(
            "/test-bucket/folder/object.txt"
        ));
    }

    #[tokio::test]
    async fn test_require_initialized_cluster_allows_exempt_paths_when_uninitialized() {
        use axum::{Router, body::Body, routing::get};
        use tower::ServiceExt;

        let (state, _temp_dir) = crate::test_helpers::test_setup_uninitialized().await;

        // Verify cluster is uninitialized
        assert!(!state.raft_node.is_initialized());

        async fn handler() -> &'static str {
            "OK"
        }

        let app = Router::new()
            .route("/health", get(handler))
            .route("/health/ready", get(handler))
            .route("/metrics", get(handler))
            .route("/cluster/status", get(handler))
            .route("/cluster/initialize", get(handler))
            .route("/cluster/members", get(handler))
            .route("/cluster/members/promote", get(handler))
            .route("/cluster/members/{node_id}", get(handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_initialized_cluster,
            ))
            .with_state(state);

        // All exempt paths should return 200 OK
        for path in [
            "/health",
            "/health/ready",
            "/metrics",
            "/cluster/status",
            "/cluster/initialize",
            "/cluster/members",
            "/cluster/members/promote",
            "/cluster/members/1",
        ] {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Exempt path {} should return 200 OK when uninitialized",
                path
            );
        }
    }

    #[tokio::test]
    async fn test_require_initialized_cluster_rejects_s3_paths_when_uninitialized() {
        use axum::{Router, body::Body, routing::get};
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let (state, _temp_dir) = crate::test_helpers::test_setup_uninitialized().await;

        // Verify cluster is uninitialized
        assert!(!state.raft_node.is_initialized());

        async fn handler() -> &'static str {
            "OK"
        }

        let app = Router::new()
            .route("/", get(handler))
            .route("/{bucket}", get(handler))
            .route("/{bucket}/{key}", get(handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_initialized_cluster,
            ))
            .with_state(state);

        // Non-exempt paths should return 503 Service Unavailable
        for path in ["/", "/my-bucket", "/my-bucket/my-key"] {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "Non-exempt path {} should return 503 when uninitialized",
                path
            );

            // Verify Content-Type header
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).unwrap(),
                "application/xml"
            );

            // Verify x-amz-request-id header is present
            assert!(
                response.headers().contains_key("x-amz-request-id"),
                "Response should have x-amz-request-id header"
            );

            // Verify XML body contains S3 error
            let body = response.into_body().collect().await.unwrap().to_bytes();
            let body_str = String::from_utf8_lossy(&body);
            assert!(
                body_str.contains("<Code>ServiceUnavailable</Code>"),
                "Response body should contain ServiceUnavailable error code"
            );
            assert!(
                body_str.contains("Cluster is not initialized"),
                "Response body should contain initialization message"
            );
        }
    }

    #[tokio::test]
    async fn test_require_initialized_cluster_allows_all_paths_when_initialized() {
        use axum::{Router, body::Body, routing::get};
        use tower::ServiceExt;

        let (state, _temp_dir) = crate::test_helpers::test_setup_empty().await;

        // Verify cluster is initialized
        assert!(state.raft_node.is_initialized());

        async fn handler() -> &'static str {
            "OK"
        }

        let app = Router::new()
            .route("/", get(handler))
            .route("/health", get(handler))
            .route("/{bucket}", get(handler))
            .route("/{bucket}/{key}", get(handler))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                require_initialized_cluster,
            ))
            .with_state(state);

        // All paths should return 200 OK when initialized
        for path in ["/", "/health", "/my-bucket", "/my-bucket/my-key"] {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();

            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Path {} should return 200 OK when initialized",
                path
            );
        }
    }
}
