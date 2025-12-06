use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use chrono::Utc;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{error, info, instrument, warn};

use super::{ComponentHealth, ComponentStatus, ReadinessResponse};
use crate::metrics;
use crate::state::AppState;

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
        let _ = data_path;
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
pub async fn readiness_check(State(state): State<AppState>) -> impl IntoResponse {
    info!("Readiness check requested");

    let mut components = HashMap::new();

    components.insert("rocksdb".to_string(), check_rocksdb(&state).await);
    components.insert("filesystem".to_string(), check_filesystem(&state).await);
    components.insert("disk_space".to_string(), check_disk_space(&state).await);
    components.insert("gc_worker".to_string(), check_gc_worker(&state).await);

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

    let status_code = if overall_status == ComponentStatus::Unhealthy {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };

    (status_code, Json(response))
}
