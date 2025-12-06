mod liveness;
mod metrics;
mod readiness;

pub use liveness::health_check;
pub use metrics::metrics_handler;
pub use readiness::readiness_check;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: DateTime<Utc>,
    pub uptime_seconds: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

#[derive(Serialize)]
pub struct ComponentHealth {
    pub status: ComponentStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<HashMap<String, String>>,
}

#[derive(Serialize)]
pub struct ReadinessResponse {
    pub status: ComponentStatus,
    pub timestamp: DateTime<Utc>,
    pub components: HashMap<String, ComponentHealth>,
}
