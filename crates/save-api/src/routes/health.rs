use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::get};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{error, info, instrument, warn};

use crate::metrics;
use crate::state::AppState;

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

#[instrument(skip(state))]
async fn health_check(State(state): State<AppState>) -> Json<HealthResponse> {
    let uptime = state.start_time.elapsed().as_secs();
    info!(uptime_seconds = uptime, "Health check requested");

    Json(HealthResponse {
        status: "ok".to_string(),
        timestamp: Utc::now(),
        uptime_seconds: uptime,
    })
}

#[instrument(skip(_state))]
async fn metrics_handler(State(_state): State<AppState>) -> impl IntoResponse {
    info!("Metrics endpoint requested");

    match metrics::encode_metrics() {
        Ok(metrics) => (StatusCode::OK, metrics).into_response(),
        Err(e) => {
            error!("Failed to encode metrics: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn check_rocksdb(state: &AppState) -> ComponentHealth {
    match state.metadata.list_buckets().await {
        Ok(_) => ComponentHealth {
            status: ComponentStatus::Healthy,
            message: Some("RocksDB operational".to_string()),
            details: None,
        },
        Err(e) => {
            error!("RocksDB health check failed: {}", e);
            ComponentHealth {
                status: ComponentStatus::Unhealthy,
                message: Some(format!("RocksDB error: {}", e)),
                details: None,
            }
        }
    }
}

async fn check_filesystem(state: &AppState) -> ComponentHealth {
    let test_path = Path::new(&state.config.storage.data_path).join(".health_check");

    match fs::write(&test_path, b"health_check") {
        Ok(_) => {
            let _ = fs::remove_file(&test_path);
            ComponentHealth {
                status: ComponentStatus::Healthy,
                message: Some("Filesystem writable".to_string()),
                details: None,
            }
        }
        Err(e) => {
            error!("Filesystem health check failed: {}", e);
            ComponentHealth {
                status: ComponentStatus::Unhealthy,
                message: Some(format!("Filesystem error: {}", e)),
                details: None,
            }
        }
    }
}

async fn check_disk_space(state: &AppState) -> ComponentHealth {
    let data_path = Path::new(&state.config.storage.data_path);

    #[cfg(target_family = "unix")]
    {
        match nix::sys::statvfs::statvfs(data_path) {
            Ok(stat) => {
                let total_bytes = stat.blocks() * stat.block_size();
                let available_bytes = stat.blocks_available() * stat.block_size();
                let used_bytes = total_bytes - available_bytes;
                let used_percent = (used_bytes as f64 / total_bytes as f64) * 100.0;
                let available_percent = 100.0 - used_percent;

                let mut details = HashMap::new();
                details.insert(
                    "total_gb".to_string(),
                    format!("{:.2}", total_bytes as f64 / 1_000_000_000.0),
                );
                details.insert(
                    "available_gb".to_string(),
                    format!("{:.2}", available_bytes as f64 / 1_000_000_000.0),
                );
                details.insert("used_percent".to_string(), format!("{:.1}", used_percent));

                let (status, message) = if available_percent < 5.0 {
                    (
                        ComponentStatus::Unhealthy,
                        "Critical: Less than 5% disk space available",
                    )
                } else if available_percent < 10.0 {
                    (
                        ComponentStatus::Degraded,
                        "Warning: Less than 10% disk space available",
                    )
                } else {
                    (ComponentStatus::Healthy, "Sufficient disk space available")
                };

                if status != ComponentStatus::Healthy {
                    warn!(
                        available_percent = %available_percent,
                        "Disk space {}",
                        if status == ComponentStatus::Unhealthy { "critical" } else { "low" }
                    );
                }

                ComponentHealth {
                    status,
                    message: Some(message.to_string()),
                    details: Some(details),
                }
            }
            Err(e) => {
                error!("Failed to check disk space: {}", e);
                ComponentHealth {
                    status: ComponentStatus::Degraded,
                    message: Some(format!("Could not check disk space: {}", e)),
                    details: None,
                }
            }
        }
    }

    #[cfg(not(target_family = "unix"))]
    {
        ComponentHealth {
            status: ComponentStatus::Healthy,
            message: Some("Disk space check not available on this platform".to_string()),
            details: None,
        }
    }
}

async fn check_gc_worker(state: &AppState) -> ComponentHealth {
    let gc_last_run = metrics::gc_last_run_seconds().get();

    if gc_last_run == 0 {
        ComponentHealth {
            status: ComponentStatus::Healthy,
            message: Some("GC worker not yet run (system recently started)".to_string()),
            details: None,
        }
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let time_since_last_run = now - gc_last_run;

        let gc_interval = state.config.storage.gc_interval_secs as i64;
        let max_expected_interval = gc_interval * 2;

        let mut details = HashMap::new();
        details.insert(
            "last_run_seconds_ago".to_string(),
            time_since_last_run.to_string(),
        );
        details.insert("gc_interval_seconds".to_string(), gc_interval.to_string());

        let (status, message) = if time_since_last_run > max_expected_interval * 2 {
            (
                ComponentStatus::Unhealthy,
                format!("GC worker hasn't run in {} seconds", time_since_last_run),
            )
        } else if time_since_last_run > max_expected_interval {
            (
                ComponentStatus::Degraded,
                format!(
                    "GC worker delayed, last run {} seconds ago",
                    time_since_last_run
                ),
            )
        } else {
            (
                ComponentStatus::Healthy,
                "GC worker operational".to_string(),
            )
        };

        if status != ComponentStatus::Healthy {
            warn!(
                time_since_last_run = %time_since_last_run,
                gc_interval = %gc_interval,
                "GC worker health issue detected"
            );
        }

        ComponentHealth {
            status,
            message: Some(message),
            details: Some(details),
        }
    }
}

#[instrument(skip(state))]
async fn readiness_check(State(state): State<AppState>) -> impl IntoResponse {
    info!("Readiness check requested");

    let mut components = HashMap::new();

    // Run all health checks
    components.insert("rocksdb".to_string(), check_rocksdb(&state).await);
    components.insert("filesystem".to_string(), check_filesystem(&state).await);
    components.insert("disk_space".to_string(), check_disk_space(&state).await);
    components.insert("gc_worker".to_string(), check_gc_worker(&state).await);

    // Determine overall status
    let overall_status = if components
        .values()
        .any(|c| c.status == ComponentStatus::Unhealthy)
    {
        ComponentStatus::Unhealthy
    } else if components
        .values()
        .any(|c| c.status == ComponentStatus::Degraded)
    {
        ComponentStatus::Degraded
    } else {
        ComponentStatus::Healthy
    };

    let response = ReadinessResponse {
        status: overall_status,
        timestamp: Utc::now(),
        components,
    };

    // Return 503 Service Unavailable if unhealthy, 200 OK otherwise
    let status_code = if overall_status == ComponentStatus::Unhealthy {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(response))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_check))
        .route("/health/ready", get(readiness_check))
        .route("/metrics", get(metrics_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_endpoint() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["status"], "ok");
        assert!(json["timestamp"].is_string());
        assert!(json["uptime_seconds"].is_number());
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let metrics_text = String::from_utf8(body.to_vec()).unwrap();

        // Verify Prometheus format
        assert!(metrics_text.contains("save_http_requests_total"));
        assert!(metrics_text.contains("save_http_request_duration_seconds"));
        assert!(metrics_text.contains("save_object_size_bytes"));
        assert!(metrics_text.contains("save_multipart_uploads_in_progress"));
    }
}
