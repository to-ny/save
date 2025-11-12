use axum::{extract::Request, middleware::Next, response::Response};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::debug;

use crate::metrics::{http_request_duration_seconds, http_requests_total};

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
    state.request_tracker.increment();
    let response = next.run(request).await;
    state.request_tracker.decrement();
    response
}

pub async fn track_metrics(request: Request, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().to_string();
    let endpoint = normalize_endpoint(request.uri().path());

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
}
