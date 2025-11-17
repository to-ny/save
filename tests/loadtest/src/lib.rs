pub mod bucket_setup;
pub mod config;
pub mod error;
pub mod metrics;
pub mod objects;
pub mod reporting;
pub mod scenarios;
pub mod signing;
pub mod system_metrics;
pub mod transactions;

use goose::metrics::GooseMetrics;
use goose::prelude::*;

pub type Result<T> = anyhow::Result<T>;

pub async fn run_scenario(
    scenario: Scenario,
    config: &config::LoadTestConfig,
) -> Result<GooseMetrics> {
    let mut goose_config = goose::config::GooseConfiguration::default();
    goose_config.host = config.target.endpoint.to_string();
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

    let metrics = GooseAttack::initialize_with_config(goose_config)?
        .register_scenario(scenario)
        .execute()
        .await?;

    Ok(metrics)
}

pub async fn run_scenario_with_report(
    scenario: Scenario,
    config: &config::LoadTestConfig,
    report_name: &str,
) -> Result<reporting::TestReport> {
    bucket_setup::ensure_bucket_exists(config).await?;

    let (prometheus_handle, system_handle, shutdown_tx) = start_metrics_collection(config).await;

    let metrics = run_scenario(scenario, config).await?;

    let (prometheus_samples, system_samples) =
        stop_metrics_collection(prometheus_handle, system_handle, shutdown_tx).await;

    let target = reporting::TargetEnvironment::detect(config.target.endpoint.clone())?;

    let report = reporting::TestReport::from_goose_metrics(
        report_name,
        &metrics,
        target,
        prometheus_samples,
        system_samples,
    );

    let writer = reporting::ReportWriter::new(config.reporting.output_dir.clone());
    writer.save(&report)?;

    Ok(report)
}

async fn start_metrics_collection(
    config: &config::LoadTestConfig,
) -> (
    Option<tokio::task::JoinHandle<Vec<metrics::PrometheusSnapshot>>>,
    Option<tokio::task::JoinHandle<Vec<system_metrics::SystemMetrics>>>,
    tokio::sync::broadcast::Sender<()>,
) {
    let (shutdown_tx, _) = tokio::sync::broadcast::channel(1);

    let prometheus_handle = if let Some(endpoint) = &config.reporting.prometheus_endpoint {
        let endpoint = endpoint.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        Some(tokio::spawn(async move {
            let collector = match metrics::MetricsCollector::new(endpoint) {
                Ok(c) => c,
                Err(_) => return Vec::new(),
            };
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

async fn stop_metrics_collection(
    prometheus_handle: Option<tokio::task::JoinHandle<Vec<metrics::PrometheusSnapshot>>>,
    system_handle: Option<tokio::task::JoinHandle<Vec<system_metrics::SystemMetrics>>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
) -> (
    Vec<metrics::PrometheusSnapshot>,
    Vec<system_metrics::SystemMetrics>,
) {
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

pub fn load_config() -> Result<config::LoadTestConfig> {
    let config_path =
        std::env::var("LOADTEST_CONFIG").unwrap_or_else(|_| "config.toml".to_string());

    if std::path::Path::new(&config_path).exists() {
        Ok(config::LoadTestConfig::from_file(&config_path)?)
    } else {
        Ok(config::LoadTestConfig::load()?)
    }
}

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
