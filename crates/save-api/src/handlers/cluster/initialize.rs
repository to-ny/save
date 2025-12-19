use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use save_common::cluster::parse_peer;
use std::time::Duration;
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

    let members: Vec<(u64, String, String, String)> = match request
        .members
        .iter()
        .map(|m| {
            parse_peer(m)
                .map(|info| {
                    (
                        info.node_id,
                        info.raft_addr(),
                        info.http_addr(),
                        info.replication_addr(),
                    )
                })
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

    let my_node_id = state.raft_node.node_id();

    // Find this node's addresses from the members list
    let (my_raft_addr, my_http_addr, my_replication_addr) =
        match members.iter().find(|(id, _, _, _)| *id == my_node_id) {
            Some((_, raft_addr, http_addr, replication_addr)) => (
                raft_addr.clone(),
                http_addr.clone(),
                replication_addr.clone(),
            ),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(InitializeResponse {
                        success: false,
                        message: format!(
                            "This node (id={}) must be included in members list",
                            my_node_id
                        ),
                    }),
                )
                    .into_response();
            }
        };

    // Step 1: Initialize this node as a single-node cluster first.
    // This allows this node to become leader immediately.
    let initial_member = vec![(my_node_id, my_raft_addr, my_http_addr, my_replication_addr)];
    if let Err(e) = state.raft_node.initialize(initial_member).await {
        error!("Failed to initialize single-node cluster: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(InitializeResponse {
                success: false,
                message: format!("Failed to initialize: {}", e),
            }),
        )
            .into_response();
    }

    info!("Initialized as single-node cluster, waiting to become leader");

    // Step 2: Wait for this node to become leader
    if let Err(e) = state
        .raft_node
        .wait_for_leader(Duration::from_secs(5))
        .await
    {
        error!("Failed to become leader: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(InitializeResponse {
                success: false,
                message: format!("Failed to become leader: {}", e),
            }),
        )
            .into_response();
    }

    info!("Became leader, adding other nodes");

    // Step 3: Add other nodes as learners and promote to voters
    let other_members: Vec<(u64, String, String, String)> = members
        .into_iter()
        .filter(|(id, _, _, _)| *id != my_node_id)
        .collect();

    if !other_members.is_empty() {
        // Add all other nodes as learners first
        for (node_id, raft_addr, http_addr, replication_addr) in &other_members {
            if let Err(e) = state
                .raft_node
                .add_learner(
                    *node_id,
                    raft_addr.clone(),
                    http_addr.clone(),
                    replication_addr.clone(),
                )
                .await
            {
                error!("Failed to add learner {}: {}", node_id, e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(InitializeResponse {
                        success: false,
                        message: format!("Failed to add learner {}: {}", node_id, e),
                    }),
                )
                    .into_response();
            }
            info!("Added node {} as learner", node_id);
        }

        // Small delay to let learners sync
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Promote all learners to voters
        let learner_ids: Vec<u64> = other_members.iter().map(|(id, _, _, _)| *id).collect();
        if let Err(e) = state.raft_node.promote_voters(learner_ids.clone()).await {
            error!("Failed to promote voters: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(InitializeResponse {
                    success: false,
                    message: format!("Failed to promote voters: {}", e),
                }),
            )
                .into_response();
        }
        info!("Promoted {} learners to voters", learner_ids.len());
    }

    info!(
        "Cluster initialized successfully with {} members",
        other_members.len() + 1
    );
    (
        StatusCode::OK,
        Json(InitializeResponse {
            success: true,
            message: "Cluster initialized".to_string(),
        }),
    )
        .into_response()
}
