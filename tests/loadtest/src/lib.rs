pub mod config;
pub mod metrics;
pub mod objects;
pub mod reporting;
pub mod scenarios;
pub mod signing;
pub mod system_metrics;
pub mod transactions;

use goose::metrics::GooseMetrics;
use goose::prelude::*;
use std::sync::Arc;
use std::sync::LazyLock;
use tokio::sync::RwLock;
use transactions::AppState;

pub static GLOBAL_STATE: LazyLock<Arc<RwLock<Option<Arc<AppState>>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

/// Result type for load test operations
pub type Result<T> = anyhow::Result<T>;

/// Run a load test scenario and return metrics
pub async fn run_scenario(
    scenario: Scenario,
    config: &config::LoadTestConfig,
) -> Result<GooseMetrics> {
    // Initialize global state - replace if already exists (for test isolation)
    let app_state = Arc::new(AppState::new(config.clone()));
    {
        let mut state = GLOBAL_STATE.write().await;
        *state = Some(app_state);
    }

    // Build Goose configuration programmatically (no CLI parsing)
    let mut goose_config = goose::config::GooseConfiguration::default();
    goose_config.host = config.target.endpoint.clone();
    goose_config.run_time = format!("{}s", config.workload.duration_secs);
    goose_config.users = Some(config.workload.users.max);
    goose_config.startup_time = format!(
        "{}s",
        (config.workload.users.max / config.workload.users.hatch_rate).max(1)
    );
    goose_config.no_metrics = false;
    goose_config.no_reset_metrics = false;
    goose_config.no_error_summary = false;
    goose_config.timeout = Some("120".to_string());

    // Build and execute attack
    let metrics = GooseAttack::initialize_with_config(goose_config)?
        .register_scenario(scenario)
        .execute()
        .await?;

    Ok(metrics)
}

/// Run a scenario and return metrics with assertions for success
pub async fn run_scenario_with_report(
    scenario: Scenario,
    config: &config::LoadTestConfig,
    report_name: &str,
) -> Result<reporting::TestReport> {
    // Start metrics collection tasks if configured
    let (prometheus_handle, system_handle, shutdown_tx) = start_metrics_collection(config).await;

    // Run the load test
    let metrics = run_scenario(scenario, config).await?;

    // Stop metrics collection and gather samples
    let (prometheus_samples, system_samples) =
        stop_metrics_collection(prometheus_handle, system_handle, shutdown_tx).await;

    let report = reporting::TestReport::from_goose_metrics(
        report_name,
        &metrics,
        prometheus_samples,
        system_samples,
    );

    // Save reports if configured
    std::fs::create_dir_all(&config.reporting.output_dir)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let json_path = format!(
        "{}/{}-{}.json",
        config.reporting.output_dir, report_name, timestamp
    );
    report.save_json(&json_path)?;

    let md_path = format!(
        "{}/{}-{}.md",
        config.reporting.output_dir, report_name, timestamp
    );
    report.save_markdown(&md_path)?;

    Ok(report)
}

/// Start background metrics collection tasks
async fn start_metrics_collection(
    config: &config::LoadTestConfig,
) -> (
    Option<tokio::task::JoinHandle<Vec<metrics::PrometheusMetrics>>>,
    Option<tokio::task::JoinHandle<Vec<system_metrics::SystemMetrics>>>,
    tokio::sync::broadcast::Sender<()>,
) {
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    let prometheus_handle = if let Some(endpoint) = &config.reporting.prometheus_endpoint {
        let endpoint = endpoint.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        Some(tokio::spawn(async move {
            let collector = metrics::MetricsCollector::new(endpoint);
            let mut samples = Vec::new();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Ok(sample) = collector.collect().await {
                            samples.push(sample);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
            samples
        }))
    } else {
        None
    };

    let system_handle = if config.reporting.collect_system_metrics {
        let mut shutdown_rx = shutdown_tx.subscribe();
        Some(tokio::spawn(async move {
            let pid = std::process::id();
            let mut collector = system_metrics::SystemCollector::new(pid);
            let mut samples = Vec::new();
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Some(sample) = collector.collect() {
                            samples.push(sample);
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
            samples
        }))
    } else {
        None
    };

    (prometheus_handle, system_handle, shutdown_tx)
}

/// Stop metrics collection and return collected samples
async fn stop_metrics_collection(
    prometheus_handle: Option<tokio::task::JoinHandle<Vec<metrics::PrometheusMetrics>>>,
    system_handle: Option<tokio::task::JoinHandle<Vec<system_metrics::SystemMetrics>>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> (
    Vec<metrics::PrometheusMetrics>,
    Vec<system_metrics::SystemMetrics>,
) {
    // Signal shutdown to all collection tasks
    let _ = shutdown_tx.send(());

    let prometheus_samples = if let Some(handle) = prometheus_handle {
        handle.await.unwrap_or_default()
    } else {
        Vec::new()
    };

    let system_samples = if let Some(handle) = system_handle {
        handle.await.unwrap_or_default()
    } else {
        Vec::new()
    };

    (prometheus_samples, system_samples)
}

/// Helper to load config from environment or use default path
pub fn load_config() -> Result<config::LoadTestConfig> {
    let config_path =
        std::env::var("LOADTEST_CONFIG").unwrap_or_else(|_| "./config.toml".to_string());

    if std::path::Path::new(&config_path).exists() {
        config::LoadTestConfig::from_file(&config_path)
    } else {
        Ok(config::LoadTestConfig::default())
    }
}

/// Helper to build scenario from name
pub fn build_scenario_by_name(name: &str, config: &config::LoadTestConfig) -> Result<Scenario> {
    let scenario = match name.to_lowercase().as_str() {
        "read-heavy" | "read_heavy" => scenarios::read_heavy::build_scenario(config),
        "write-heavy" | "write_heavy" => scenarios::write_heavy::build_scenario(config),
        "mixed" => scenarios::mixed::build_scenario(config),
        "multipart" => scenarios::multipart::build_scenario(config),
        _ => anyhow::bail!(
            "Unknown scenario: {}. Valid options: read-heavy, write-heavy, mixed, multipart",
            name
        ),
    };
    Ok(scenario)
}
