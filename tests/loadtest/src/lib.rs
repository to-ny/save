pub mod aggregation;
pub mod benchmark;
pub mod bucket_setup;
pub mod config;
pub mod error;
pub mod metrics;
pub mod objects;
pub mod report;
pub mod reporting;
pub mod signing;
pub mod system_metrics;
pub mod workload;

use chrono::Utc;
use serde::Deserialize;
use url::Url;

pub type Result<T> = anyhow::Result<T>;

pub async fn run_benchmark(
    workload_type: &str,
    config: &config::LoadTestConfig,
) -> Result<report::BenchmarkReport> {
    bucket_setup::ensure_bucket_exists(config).await?;

    let start_time = Utc::now();
    let executor = workload::WorkloadExecutor::new(config.clone());

    let metrics = match workload_type {
        "mixed" => executor.run_mixed_workload().await?,
        "write-heavy" => executor.run_write_heavy_workload().await?,
        "read-heavy" => executor.run_read_heavy_workload().await?,
        _ => anyhow::bail!("Unknown workload type: {}", workload_type),
    };

    workload::print_progress(&metrics, config.workload.duration_secs);

    let aggregated =
        aggregation::AggregatedMetrics::from_metrics(&metrics, config.workload.duration_secs);

    let target = reporting::TargetEnvironment::detect(config.target.endpoint.clone())?;

    let report =
        report::BenchmarkReport::new(workload_type, config, aggregated, target, start_time);

    let output_path = report.save(&config.reporting.output_dir)?;
    println!("\nReport saved to: {}", output_path.display());

    Ok(report)
}

pub async fn run_benchmark_with_metrics(
    workload_type: &str,
    config: &config::LoadTestConfig,
    metrics_interval_secs: u64,
) -> Result<report::BenchmarkReport> {
    bucket_setup::ensure_bucket_exists(config).await?;

    let (prometheus_handle, system_handle, shutdown_tx) =
        start_metrics_collection(config, metrics_interval_secs).await;

    let start_time = Utc::now();
    let executor = workload::WorkloadExecutor::new(config.clone());

    let metrics = match workload_type {
        "mixed" => executor.run_mixed_workload().await?,
        "write-heavy" => executor.run_write_heavy_workload().await?,
        "read-heavy" => executor.run_read_heavy_workload().await?,
        _ => anyhow::bail!("Unknown workload type: {}", workload_type),
    };

    let (_prometheus_samples, _system_samples) =
        stop_metrics_collection(prometheus_handle, system_handle, shutdown_tx).await;

    workload::print_progress(&metrics, config.workload.duration_secs);

    let aggregated =
        aggregation::AggregatedMetrics::from_metrics(&metrics, config.workload.duration_secs);

    let target = reporting::TargetEnvironment::detect(config.target.endpoint.clone())?;

    let report =
        report::BenchmarkReport::new(workload_type, config, aggregated, target, start_time);

    let output_path = report.save(&config.reporting.output_dir)?;
    println!("\nReport saved to: {}", output_path.display());

    Ok(report)
}

async fn start_metrics_collection(
    config: &config::LoadTestConfig,
    interval_secs: u64,
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
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));

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
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));

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

fn serialize_url<S>(url: &Url, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(url.as_str())
}

fn deserialize_url<'de, D>(deserializer: D) -> std::result::Result<Url, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}
