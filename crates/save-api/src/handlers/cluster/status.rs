use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::ClusterStatusResponse;
use crate::state::AppState;

pub async fn cluster_status(State(state): State<AppState>) -> impl IntoResponse {
    let status = state.raft_node.get_status();
    (StatusCode::OK, Json(ClusterStatusResponse { status }))
}
