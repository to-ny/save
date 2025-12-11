use axum::{Json, extract::State};
use chrono::Utc;
use tracing::{debug, instrument};

use super::HealthResponse;
use crate::state::AppState;

#[instrument(skip(state))]
pub async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    debug!(uptime_seconds = uptime, "Health check requested");

    Json(HealthResponse {
        status: "ok".to_string(),
        timestamp: Utc::now(),
        uptime_seconds: uptime,
    })
}
