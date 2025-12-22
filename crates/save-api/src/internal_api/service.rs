//! Cluster admin service implementation.

use crate::AppState;
use save_metadata::raft::RaftState;
use save_proto::cluster as proto;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Validates an AddLearnerRequest, returning an error message if invalid.
fn validate_add_learner_request(req: &proto::AddLearnerRequest) -> Option<&'static str> {
    if req.node_id == 0 {
        return Some("node_id must be > 0");
    }
    if req.raft_address.is_empty() {
        return Some("raft_address cannot be empty");
    }
    if req.http_address.is_empty() {
        return Some("http_address cannot be empty");
    }
    if req.replication_address.is_empty() {
        return Some("replication_address cannot be empty");
    }
    None
}

/// Service handling cluster administration operations.
#[derive(Clone)]
pub struct ClusterAdminService {
    pub(super) state: AppState,
}

impl ClusterAdminService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn get_status(&self) -> proto::GetStatusResponse {
        let status = self.state.raft_node.get_status();

        proto::GetStatusResponse {
            node_id: status.node_id,
            initialized: status.initialized,
            state: raft_state_to_proto(status.state).into(),
            current_term: status.current_term,
            leader_id: status.current_leader,
            voters: status.voters,
            learners: status.learners,
            last_applied_index: status.last_applied_index,
            last_log_index: status.last_log_index,
        }
    }

    pub async fn add_learner(&self, req: proto::AddLearnerRequest) -> proto::AddLearnerResponse {
        if let Some(error_message) = validate_add_learner_request(&req) {
            return proto::AddLearnerResponse {
                success: false,
                error_message: error_message.to_string(),
            };
        }

        info!(node_id = req.node_id, raft_addr = %req.raft_address, http_addr = %req.http_address, replication_addr = %req.replication_address, "Adding learner node");

        match self
            .state
            .raft_node
            .add_learner(
                req.node_id,
                req.raft_address.clone(),
                req.http_address.clone(),
                req.replication_address.clone(),
            )
            .await
        {
            Ok(()) => {
                info!(node_id = req.node_id, "Learner node added successfully");
                proto::AddLearnerResponse {
                    success: true,
                    error_message: String::new(),
                }
            }
            Err(e) => {
                warn!(node_id = req.node_id, error = %e, "Failed to add learner");
                proto::AddLearnerResponse {
                    success: false,
                    error_message: e.to_string(),
                }
            }
        }
    }

    pub async fn promote_voters(
        &self,
        req: proto::PromoteVotersRequest,
    ) -> proto::PromoteVotersResponse {
        if req.node_ids.is_empty() {
            return proto::PromoteVotersResponse {
                success: false,
                error_message: "node_ids cannot be empty".to_string(),
            };
        }

        info!(node_ids = ?req.node_ids, "Promoting learners to voters");

        match self
            .state
            .raft_node
            .promote_voters(req.node_ids.clone())
            .await
        {
            Ok(()) => {
                info!(node_ids = ?req.node_ids, "Learners promoted successfully");
                proto::PromoteVotersResponse {
                    success: true,
                    error_message: String::new(),
                }
            }
            Err(e) => {
                warn!(node_ids = ?req.node_ids, error = %e, "Failed to promote learners");
                proto::PromoteVotersResponse {
                    success: false,
                    error_message: e.to_string(),
                }
            }
        }
    }

    pub async fn remove_node(&self, req: proto::RemoveNodeRequest) -> proto::RemoveNodeResponse {
        if req.node_id == 0 {
            return proto::RemoveNodeResponse {
                success: false,
                error_message: "node_id must be > 0".to_string(),
            };
        }

        let my_node_id = self.state.raft_node.node_id();
        if req.node_id == my_node_id && self.state.raft_node.is_leader().await {
            return proto::RemoveNodeResponse {
                success: false,
                error_message: "Cannot remove the leader node. Transfer leadership first."
                    .to_string(),
            };
        }

        info!(node_id = req.node_id, "Removing node from cluster");

        match self.state.raft_node.remove_node(req.node_id).await {
            Ok(()) => {
                info!(node_id = req.node_id, "Node removed successfully");
                proto::RemoveNodeResponse {
                    success: true,
                    error_message: String::new(),
                }
            }
            Err(e) => {
                warn!(node_id = req.node_id, error = %e, "Failed to remove node");
                proto::RemoveNodeResponse {
                    success: false,
                    error_message: e.to_string(),
                }
            }
        }
    }

    pub async fn drain_node(&self, req: proto::DrainNodeRequest) -> proto::DrainNodeResponse {
        let timeout_secs = if req.timeout_secs == 0 {
            self.state.config.shutdown.drain_timeout_secs
        } else {
            req.timeout_secs
        };

        info!(timeout_secs, "Starting node drain");

        let drain_timeout = Duration::from_secs(timeout_secs);
        let start = tokio::time::Instant::now();
        let mut drained = 0u64;

        loop {
            let in_flight = self.state.request_tracker.in_flight_count();
            if in_flight == 0 {
                info!(drained_requests = drained, "Node drain complete");
                return proto::DrainNodeResponse {
                    success: true,
                    error_message: String::new(),
                    drained_requests: drained,
                };
            }

            if start.elapsed() >= drain_timeout {
                warn!(
                    remaining_requests = in_flight,
                    drained_requests = drained,
                    "Drain timeout reached"
                );
                return proto::DrainNodeResponse {
                    success: false,
                    error_message: format!("Drain timeout: {} requests still in flight", in_flight),
                    drained_requests: drained,
                };
            }

            let prev_in_flight = in_flight;
            tokio::time::sleep(Duration::from_millis(100)).await;
            let new_in_flight = self.state.request_tracker.in_flight_count();
            if new_in_flight < prev_in_flight {
                drained += (prev_in_flight - new_in_flight) as u64;
            }
        }
    }

    pub async fn get_debug_info(
        &self,
        _req: proto::GetDebugInfoRequest,
    ) -> proto::GetDebugInfoResponse {
        let status = self.state.raft_node.get_status();

        let node_info = serde_json::json!({
            "node_id": status.node_id,
            "initialized": status.initialized,
            "state": format!("{:?}", status.state),
            "current_term": status.current_term,
            "leader_id": status.current_leader,
            "last_applied_index": status.last_applied_index,
            "last_log_index": status.last_log_index,
            "in_flight_requests": self.state.request_tracker.in_flight_count(),
        });

        let cluster_info = serde_json::json!({
            "voters": status.voters,
            "learners": status.learners,
            "voters_count": status.voters.len(),
            "learners_count": status.learners.len(),
        });

        // TODO: Add recent log entries if requested
        let recent_logs = vec![];

        debug!("Debug info requested");

        proto::GetDebugInfoResponse {
            node_info: node_info.to_string(),
            cluster_info: cluster_info.to_string(),
            recent_logs,
        }
    }

    pub async fn trigger_elect(
        &self,
        _req: proto::TriggerElectRequest,
    ) -> proto::TriggerElectResponse {
        let node_id = self.state.raft_node.node_id();
        info!(node_id = node_id, "Triggering election");

        match self.state.raft_node.trigger_elect().await {
            Ok(()) => {
                info!(node_id = node_id, "Election triggered successfully");
                proto::TriggerElectResponse {
                    success: true,
                    error_message: String::new(),
                }
            }
            Err(e) => {
                warn!(node_id = node_id, error = %e, "Failed to trigger election");
                proto::TriggerElectResponse {
                    success: false,
                    error_message: e.to_string(),
                }
            }
        }
    }
}

fn raft_state_to_proto(state: RaftState) -> proto::RaftState {
    match state {
        RaftState::Leader => proto::RaftState::Leader,
        RaftState::Follower => proto::RaftState::Follower,
        RaftState::Candidate => proto::RaftState::Candidate,
        RaftState::Learner => proto::RaftState::Learner,
        RaftState::Shutdown => proto::RaftState::Shutdown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raft_state_to_proto() {
        assert_eq!(
            raft_state_to_proto(RaftState::Leader),
            proto::RaftState::Leader
        );
        assert_eq!(
            raft_state_to_proto(RaftState::Follower),
            proto::RaftState::Follower
        );
        assert_eq!(
            raft_state_to_proto(RaftState::Candidate),
            proto::RaftState::Candidate
        );
        assert_eq!(
            raft_state_to_proto(RaftState::Learner),
            proto::RaftState::Learner
        );
        assert_eq!(
            raft_state_to_proto(RaftState::Shutdown),
            proto::RaftState::Shutdown
        );
    }

    #[tokio::test]
    async fn test_get_status() {
        let (state, _tmp) = crate::test_helpers::test_setup_empty().await;
        let service = ClusterAdminService::new(state);

        let response = service.get_status().await;

        assert_eq!(response.node_id, 1);
        assert!(response.initialized);
        assert!(!response.voters.is_empty());
    }

    // Unit tests for validate_add_learner_request (synchronous, no setup needed)
    #[test]
    fn test_validate_add_learner_valid_request() {
        let req = proto::AddLearnerRequest {
            node_id: 2,
            raft_address: "http://127.0.0.1:9001".to_string(),
            http_address: "http://127.0.0.1:9000".to_string(),
            replication_address: "http://127.0.0.1:9002".to_string(),
        };
        assert!(validate_add_learner_request(&req).is_none());
    }

    #[test]
    fn test_validate_add_learner_invalid_node_id() {
        let req = proto::AddLearnerRequest {
            node_id: 0,
            raft_address: "http://127.0.0.1:9001".to_string(),
            http_address: "http://127.0.0.1:9000".to_string(),
            replication_address: "http://127.0.0.1:9002".to_string(),
        };
        assert_eq!(
            validate_add_learner_request(&req),
            Some("node_id must be > 0")
        );
    }

    #[test]
    fn test_validate_add_learner_empty_addresses() {
        // Empty raft_address
        let req = proto::AddLearnerRequest {
            node_id: 2,
            raft_address: String::new(),
            http_address: "http://127.0.0.1:9000".to_string(),
            replication_address: "http://127.0.0.1:9002".to_string(),
        };
        assert_eq!(
            validate_add_learner_request(&req),
            Some("raft_address cannot be empty")
        );

        // Empty http_address
        let req = proto::AddLearnerRequest {
            node_id: 2,
            raft_address: "http://127.0.0.1:9001".to_string(),
            http_address: String::new(),
            replication_address: "http://127.0.0.1:9002".to_string(),
        };
        assert_eq!(
            validate_add_learner_request(&req),
            Some("http_address cannot be empty")
        );

        // Empty replication_address
        let req = proto::AddLearnerRequest {
            node_id: 2,
            raft_address: "http://127.0.0.1:9001".to_string(),
            http_address: "http://127.0.0.1:9000".to_string(),
            replication_address: String::new(),
        };
        assert_eq!(
            validate_add_learner_request(&req),
            Some("replication_address cannot be empty")
        );
    }

    // Integration test verifying validation is wired up in the service
    #[tokio::test]
    async fn test_add_learner_validation_wired_up() {
        let (state, _tmp) = crate::test_helpers::test_setup_empty().await;
        let service = ClusterAdminService::new(state);

        // Test one validation failure to verify the wiring
        let req = proto::AddLearnerRequest {
            node_id: 0,
            raft_address: "http://127.0.0.1:9001".to_string(),
            http_address: "http://127.0.0.1:9000".to_string(),
            replication_address: "http://127.0.0.1:9002".to_string(),
        };
        let response = service.add_learner(req).await;

        assert!(!response.success);
        assert!(response.error_message.contains("node_id must be > 0"));
    }

    #[tokio::test]
    async fn test_promote_voters_empty_node_ids() {
        let (state, _tmp) = crate::test_helpers::test_setup_empty().await;
        let service = ClusterAdminService::new(state);

        let req = proto::PromoteVotersRequest { node_ids: vec![] };
        let response = service.promote_voters(req).await;

        assert!(!response.success);
        assert!(response.error_message.contains("node_ids cannot be empty"));
    }

    #[tokio::test]
    async fn test_remove_node_invalid_node_id() {
        let (state, _tmp) = crate::test_helpers::test_setup_empty().await;
        let service = ClusterAdminService::new(state);

        let req = proto::RemoveNodeRequest { node_id: 0 };
        let response = service.remove_node(req).await;

        assert!(!response.success);
        assert!(response.error_message.contains("node_id must be > 0"));
    }

    #[tokio::test]
    async fn test_drain_node() {
        let (state, _tmp) = crate::test_helpers::test_setup_empty().await;
        let service = ClusterAdminService::new(state);

        let req = proto::DrainNodeRequest { timeout_secs: 1 };
        let response = service.drain_node(req).await;

        // No requests in flight, should succeed immediately
        assert!(response.success);
        assert_eq!(response.drained_requests, 0);
    }

    #[tokio::test]
    async fn test_get_debug_info() {
        let (state, _tmp) = crate::test_helpers::test_setup_empty().await;
        let service = ClusterAdminService::new(state);

        let req = proto::GetDebugInfoRequest {
            include_log_entries: false,
            max_log_entries: 0,
        };
        let response = service.get_debug_info(req).await;

        assert!(!response.node_info.is_empty());
        assert!(!response.cluster_info.is_empty());

        // Verify JSON is valid
        let node_info: serde_json::Value = serde_json::from_str(&response.node_info).unwrap();
        assert_eq!(node_info["node_id"], 1);

        let cluster_info: serde_json::Value = serde_json::from_str(&response.cluster_info).unwrap();
        assert!(cluster_info["voters"].is_array());
    }
}
