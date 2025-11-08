use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::{error, info, instrument};

use crate::metrics;
use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: DateTime<Utc>,
    pub uptime_seconds: u64,
}

#[instrument(skip(state))]
async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    info!(uptime_seconds = uptime, "Health check requested");

    Json(HealthResponse {
        status: "ok".to_string(),
        timestamp: Utc::now(),
        uptime_seconds: uptime,
    })
}

#[instrument]
async fn metrics_handler() -> impl IntoResponse {
    info!("Metrics endpoint requested");

    match metrics::encode_metrics() {
        Ok(metrics) => (StatusCode::OK, metrics).into_response(),
        Err(e) => {
            error!("Failed to encode metrics: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use http_body_util::BodyExt;

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

        // Verify Prometheus format
        assert!(metrics_text.contains("save_http_requests_total"));
        assert!(metrics_text.contains("save_http_request_duration_seconds"));
        assert!(metrics_text.contains("save_object_size_bytes"));
        assert!(metrics_text.contains("save_multipart_uploads_in_progress"));
    }
}
