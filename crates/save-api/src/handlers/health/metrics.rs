use axum::{extract::State, http::StatusCode, response::IntoResponse};
use tracing::{debug, error, instrument};

use crate::metrics;
use crate::state::AppState;

#[instrument(skip(_state))]
pub async fn metrics_handler(State(_state): State<AppState>) -> impl IntoResponse {
    debug!("Metrics endpoint requested");

    match metrics::encode_metrics() {
        Ok(metrics) => (StatusCode::OK, metrics).into_response(),
        Err(e) => {
            error!("Failed to encode metrics: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
