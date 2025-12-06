use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use save_common::cluster::parse_peer;
use tracing::{error, info, warn};

use super::{InitializeRequest, InitializeResponse};
use crate::state::AppState;

pub async fn cluster_initialize(
    State(state): State<AppState>,
    Json(request): Json<InitializeRequest>,
) -> impl IntoResponse {
    info!(
        "Cluster initialization requested with {} members",
        request.members.len()
    );

    if state.raft_node.is_initialized() {
        warn!("Attempted to initialize already-initialized cluster");
        return (
            StatusCode::CONFLICT,
            Json(InitializeResponse {
                success: false,
                message: "Cluster already initialized".to_string(),
            }),
        )
            .into_response();
    }

    let members: Vec<(u64, String)> = match request
        .members
        .iter()
        .map(|m| {
            parse_peer(m)
                .map(|info| (info.node_id, info.http_addr()))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(InitializeResponse {
                    success: false,
                    message: e,
                }),
            )
                .into_response();
        }
    };

    match state.raft_node.initialize(members).await {
        Ok(()) => {
            info!("Cluster initialized successfully");
            (
                StatusCode::OK,
                Json(InitializeResponse {
                    success: true,
                    message: "Cluster initialized".to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to initialize cluster: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(InitializeResponse {
                    success: false,
                    message: format!("Failed to initialize: {}", e),
                }),
            )
                .into_response()
        }
    }
}
