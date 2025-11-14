use goose::prelude::*;
use save_loadtest::config::LoadTestConfig;
use save_loadtest::metrics::MetricsCollector;
use save_loadtest::reporting::TestReport;
use save_loadtest::system_metrics::SystemCollector;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config_path = std::env::var("LOADTEST_CONFIG")
        .unwrap_or_else(|_| "tests/loadtest/config.toml".to_string());
    let duration_secs: u64 = std::env::var("LOADTEST_DURATION")
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(3600);
    let users: usize = std::env::var("LOADTEST_USERS")
        .ok()
        .and_then(|u| u.parse().ok())
        .unwrap_or(10);
    let metrics_interval_secs: u64 = std::env::var("LOADTEST_METRICS_INTERVAL")
        .ok()
        .and_then(|i| i.parse().ok())
        .unwrap_or(30);

    println!("🔥 Save Soak Test");
    println!("   Config: {}", config_path);
    println!(
        "   Duration: {}s ({}h {}m)",
        duration_secs,
        duration_secs / 3600,
        (duration_secs % 3600) / 60
    );
    println!("   Users: {}", users);
    println!("   Metrics interval: {}s", metrics_interval_secs);
    println!();

    let mut config = LoadTestConfig::from_file(&config_path)?;
    config.workload.duration_secs = duration_secs;
    config.workload.users.max = users;
    config.workload.users.start = users;
    config.workload.users.hatch_rate = 1;

    let app_state = Arc::new(save_loadtest::transactions::AppState::new(config.clone()));

    let scenario = save_loadtest::scenarios::mixed::build_scenario(&config);

    let prometheus_samples = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let system_samples = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let metrics_task = if let Some(prom_endpoint) = &config.reporting.prometheus_endpoint {
        let collector = MetricsCollector::new(prom_endpoint.clone());
        let prom_samples = prometheus_samples.clone();
        let sys_samples = system_samples.clone();
        let collect_sys = config.reporting.collect_system_metrics;
        let server_pid = std::env::var("SERVER_PID")
            .ok()
            .and_then(|p| p.parse::<u32>().ok());

        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(metrics_interval_secs));
            let mut sys_collector = server_pid.map(SystemCollector::new);

            loop {
                interval.tick().await;

                if let Ok(metrics) = collector.collect().await {
                    prom_samples.lock().await.push(metrics);
                }

                if collect_sys
                    && let Some(ref mut collector) = sys_collector
                    && let Some(sys) = collector.collect()
                {
                    sys_samples.lock().await.push(sys);
                }
            }
        }))
    } else {
        None
    };

    save_loadtest::GLOBAL_STATE.set(app_state.clone()).ok();

    let runtime_str = format!("{}s", duration_secs);
    let metrics = GooseAttack::initialize()?
        .register_scenario(scenario)
        .set_default(GooseDefault::Host, config.target.endpoint.as_str())?
        .set_default(GooseDefault::RunTime, runtime_str.as_str())?
        .execute()
        .await?;

    if let Some(task) = metrics_task {
        task.abort();
    }

    let report = TestReport::from_goose_metrics(
        "SoakTest",
        &metrics,
        prometheus_samples.lock().await.clone(),
        system_samples.lock().await.clone(),
    );

    std::fs::create_dir_all(&config.reporting.output_dir)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let json_path = format!(
        "{}/soaktest-{}.json",
        config.reporting.output_dir, timestamp
    );
    report.save_json(&json_path)?;
    println!("\n📊 Report saved to: {}", json_path);

    let md_path = format!("{}/soaktest-{}.md", config.reporting.output_dir, timestamp);
    report.save_markdown(&md_path)?;
    println!("📊 Report saved to: {}", md_path);

    Ok(())
}
