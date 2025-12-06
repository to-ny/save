//! Health and metrics routes.

use axum::{Router, routing::get};

use crate::handlers::health::{health_check, metrics_handler, readiness_check};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_check))
        .route("/health/ready", get(readiness_check))
        .route("/metrics", get(metrics_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["timestamp"].is_string());
        assert!(json["uptime_seconds"].is_number());
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let metrics_text = String::from_utf8(body.to_vec()).unwrap();

        // Metrics are lazily registered, so in unit tests the registry may be empty.
        // Just verify we get a valid response (empty or Prometheus format).
        assert!(
            metrics_text.is_empty()
                || metrics_text.contains("# HELP")
                || metrics_text.contains("# TYPE"),
            "Expected empty or Prometheus format metrics"
        );
    }
}
