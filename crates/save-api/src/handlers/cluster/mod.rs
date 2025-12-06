mod initialize;
mod membership;
mod status;

pub use initialize::cluster_initialize;
pub use membership::{add_learner, promote_voters, remove_node};
pub use status::cluster_status;

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ClusterStatusResponse {
    #[serde(flatten)]
    pub status: save_metadata::raft::ClusterStatus,
}

#[derive(Deserialize)]
pub struct InitializeRequest {
    pub members: Vec<String>,
}

#[derive(Serialize)]
pub struct InitializeResponse {
    pub success: bool,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AddLearnerRequest {
    pub node: String,
}

#[derive(Deserialize)]
pub struct PromoteVotersRequest {
    pub node_ids: Vec<u64>,
}

#[derive(Serialize)]
pub struct MembershipResponse {
    pub success: bool,
    pub message: String,
}
