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
    let metrics = run_scenario(scenario, config).await?;

    let report = reporting::TestReport::from_goose_metrics(report_name, &metrics, vec![], vec![]);

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
