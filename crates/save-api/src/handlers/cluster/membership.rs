use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use save_common::cluster::parse_peer;
use tracing::{error, info};

use super::{AddLearnerRequest, MembershipResponse, PromoteVotersRequest};
use crate::state::AppState;

pub async fn add_learner(
    State(state): State<AppState>,
    Json(request): Json<AddLearnerRequest>,
) -> impl IntoResponse {
    info!("Adding learner node: {}", request.node);

    if !state.raft_node.is_leader().await {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            Json(MembershipResponse {
                success: false,
                message: "Not the leader. Forward request to the leader node.".to_string(),
            }),
        )
            .into_response();
    }

    let (node_id, addr) = match parse_peer(&request.node) {
        Ok(info) => (info.node_id, info.http_addr()),
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(MembershipResponse {
                    success: false,
                    message: e.to_string(),
                }),
            )
                .into_response();
        }
    };

    match state.raft_node.add_learner(node_id, addr).await {
        Ok(()) => {
            info!("Learner node {} added successfully", node_id);
            (
                StatusCode::OK,
                Json(MembershipResponse {
                    success: true,
                    message: format!("Node {} added as learner", node_id),
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to add learner: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MembershipResponse {
                    success: false,
                    message: format!("Failed to add learner: {}", e),
                }),
            )
                .into_response()
        }
    }
}

pub async fn promote_voters(
    State(state): State<AppState>,
    Json(request): Json<PromoteVotersRequest>,
) -> impl IntoResponse {
    info!("Promoting nodes to voters: {:?}", request.node_ids);

    if !state.raft_node.is_leader().await {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            Json(MembershipResponse {
                success: false,
                message: "Not the leader. Forward request to the leader node.".to_string(),
            }),
        )
            .into_response();
    }

    if request.node_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(MembershipResponse {
                success: false,
                message: "node_ids cannot be empty".to_string(),
            }),
        )
            .into_response();
    }

    match state
        .raft_node
        .promote_voters(request.node_ids.clone())
        .await
    {
        Ok(()) => {
            info!("Nodes {:?} promoted to voters", request.node_ids);
            (
                StatusCode::OK,
                Json(MembershipResponse {
                    success: true,
                    message: format!("Nodes {:?} promoted to voters", request.node_ids),
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to promote voters: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MembershipResponse {
                    success: false,
                    message: format!("Failed to promote voters: {}", e),
                }),
            )
                .into_response()
        }
    }
}

pub async fn remove_node(
    State(state): State<AppState>,
    Path(node_id): Path<u64>,
) -> impl IntoResponse {
    info!("Removing node {} from cluster", node_id);

    if !state.raft_node.is_leader().await {
        return (
            StatusCode::MISDIRECTED_REQUEST,
            Json(MembershipResponse {
                success: false,
                message: "Not the leader. Forward request to the leader node.".to_string(),
            }),
        )
            .into_response();
    }

    if node_id == state.raft_node.node_id() {
        return (
            StatusCode::BAD_REQUEST,
            Json(MembershipResponse {
                success: false,
                message: "Cannot remove the leader node. Transfer leadership first.".to_string(),
            }),
        )
            .into_response();
    }

    match state.raft_node.remove_node(node_id).await {
        Ok(()) => {
            info!("Node {} removed from cluster", node_id);
            (
                StatusCode::OK,
                Json(MembershipResponse {
                    success: true,
                    message: format!("Node {} removed from cluster", node_id),
                }),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to remove node: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MembershipResponse {
                    success: false,
                    message: format!("Failed to remove node: {}", e),
                }),
            )
                .into_response()
        }
    }
}
