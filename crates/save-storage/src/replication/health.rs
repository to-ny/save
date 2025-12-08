//! Health monitoring for replication nodes.

use super::client::ReplicationClient;
use std::time::Duration;
use tracing::debug;

/// Health status constants matching proto HealthCheckResponse.status.
pub mod status {
    pub const HEALTHY: i32 = 0;
    pub const DEGRADED: i32 = 1;
    #[allow(dead_code)]
    pub const UNHEALTHY: i32 = 2;
}

/// Default timeout for health checks.
const DEFAULT_HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Checks node health via gRPC health endpoint.
#[derive(Debug, Clone)]
pub struct HealthChecker {
    timeout: Duration,
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_HEALTH_CHECK_TIMEOUT,
        }
    }
}

impl HealthChecker {
    /// Create a health checker with custom timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    /// Get the configured timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Check if a node is healthy (HEALTHY or DEGRADED).
    pub async fn is_healthy(&self, client: &ReplicationClient) -> bool {
        match tokio::time::timeout(self.timeout, client.health_check()).await {
            Ok(Ok(resp)) => resp.status == status::HEALTHY || resp.status == status::DEGRADED,
            Ok(Err(e)) => {
                debug!(node_id = %client.node_id(), error = %e, "Health check failed");
                false
            }
            Err(_) => {
                debug!(node_id = %client.node_id(), "Health check timed out");
                false
            }
        }
    }

    /// Check health and return detailed result.
    pub async fn check(&self, client: &ReplicationClient) -> HealthCheckResult {
        match tokio::time::timeout(self.timeout, client.health_check()).await {
            Ok(Ok(resp)) => HealthCheckResult {
                node_id: client.node_id(),
                status: HealthStatus::from_proto(resp.status),
                error: None,
            },
            Ok(Err(e)) => HealthCheckResult {
                node_id: client.node_id(),
                status: HealthStatus::Unhealthy,
                error: Some(e.to_string()),
            },
            Err(_) => HealthCheckResult {
                node_id: client.node_id(),
                status: HealthStatus::Unhealthy,
                error: Some("timeout".to_string()),
            },
        }
    }
}

/// Health status of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

impl HealthStatus {
    fn from_proto(status: i32) -> Self {
        match status {
            status::HEALTHY => Self::Healthy,
            status::DEGRADED => Self::Degraded,
            _ => Self::Unhealthy,
        }
    }

    /// Returns true if the node can accept requests.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded)
    }
}

/// Result of a health check.
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub node_id: u64,
    pub status: HealthStatus,
    pub error: Option<String>,
}

impl HealthCheckResult {
    /// Returns true if the node can accept requests.
    pub fn is_available(&self) -> bool {
        self.status.is_available()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_from_proto() {
        assert_eq!(
            HealthStatus::from_proto(status::HEALTHY),
            HealthStatus::Healthy
        );
        assert_eq!(
            HealthStatus::from_proto(status::DEGRADED),
            HealthStatus::Degraded
        );
        assert_eq!(
            HealthStatus::from_proto(status::UNHEALTHY),
            HealthStatus::Unhealthy
        );
        assert_eq!(HealthStatus::from_proto(999), HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_status_is_available() {
        assert!(HealthStatus::Healthy.is_available());
        assert!(HealthStatus::Degraded.is_available());
        assert!(!HealthStatus::Unhealthy.is_available());
    }

    #[test]
    fn test_health_checker_default_timeout() {
        let checker = HealthChecker::default();
        assert_eq!(checker.timeout(), Duration::from_secs(2));
    }

    #[test]
    fn test_health_checker_custom_timeout() {
        let checker = HealthChecker::with_timeout(Duration::from_millis(500));
        assert_eq!(checker.timeout(), Duration::from_millis(500));
    }

    #[test]
    fn test_health_check_result_is_available() {
        let healthy = HealthCheckResult {
            node_id: 1,
            status: HealthStatus::Healthy,
            error: None,
        };
        assert!(healthy.is_available());

        let degraded = HealthCheckResult {
            node_id: 2,
            status: HealthStatus::Degraded,
            error: None,
        };
        assert!(degraded.is_available());

        let unhealthy = HealthCheckResult {
            node_id: 3,
            status: HealthStatus::Unhealthy,
            error: Some("connection refused".to_string()),
        };
        assert!(!unhealthy.is_available());
    }
}
