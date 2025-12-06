//! Cluster status and management endpoints.

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use save_common::cluster::parse_peer;
use save_metadata::raft::ClusterStatus;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::state::AppState;

#[derive(Serialize)]
struct ClusterStatusResponse {
    #[serde(flatten)]
    status: ClusterStatus,
}

#[derive(Serialize)]
struct ClusterDisabledResponse {
    error: String,
    cluster_enabled: bool,
}

#[derive(Deserialize)]
struct InitializeRequest {
    /// Members in format ["node_id:host:port", ...]
    members: Vec<String>,
}

#[derive(Serialize)]
struct InitializeResponse {
    success: bool,
    message: String,
}

async fn cluster_status(State(state): State<AppState>) -> impl IntoResponse {
    match &state.raft_node {
        Some(raft_node) => {
            let status = raft_node.get_status();
            (StatusCode::OK, Json(ClusterStatusResponse { status })).into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ClusterDisabledResponse {
                error: "Cluster mode not enabled".to_string(),
                cluster_enabled: false,
            }),
        )
            .into_response(),
    }
}

async fn cluster_initialize(
    State(state): State<AppState>,
    Json(request): Json<InitializeRequest>,
) -> impl IntoResponse {
    info!(
        "Cluster initialization requested with {} members",
        request.members.len()
    );

    let raft_node = match &state.raft_node {
        Some(node) => node,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(InitializeResponse {
                    success: false,
                    message: "Cluster mode not enabled".to_string(),
                }),
            )
                .into_response();
        }
    };

    // Check if already initialized
    if raft_node.is_initialized() {
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

    // Parse members using shared utility
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

    match raft_node.initialize(members).await {
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cluster/status", get(cluster_status))
        .route("/cluster/initialize", post(cluster_initialize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_cluster_status_disabled() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .uri("/cluster/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["cluster_enabled"], false);
        assert!(json["error"].as_str().unwrap().contains("not enabled"));
    }
}
