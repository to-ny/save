use axum::{extract::Request, middleware::Next, response::Response};
use save_common::tracing::{
    PARENT_SPAN_HEADER, REQUEST_ID_HEADER, TRACE_ID_HEADER, TraceContext, generate_span_id,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tracing::{Span, debug};
use uuid::Uuid;

use crate::metrics::{http_request_duration_seconds, http_requests_total};

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
}
