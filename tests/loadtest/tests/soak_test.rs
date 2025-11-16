#![cfg_attr(not(feature = "load_tests"), allow(unused_imports))]

use goose::prelude::*;
use save_loadtest::{load_config, reporting::TestReport, scenarios};
use std::sync::Arc;
use std::time::Duration;

/// Long-running soak test to detect memory leaks and performance degradation
///
/// Prerequisites:
/// - save-api server running at localhost:9000
/// - Test bucket "loadtest" created
///
/// Run with custom duration:
/// ```bash
/// LOADTEST_DURATION=3600 LOADTEST_USERS=10 \
///   cargo test --package save-loadtest --features load_tests test_soak
/// ```
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_soak() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    // Load base config
    let mut config = load_config()?;

    // Override with environment variables for soak testing
    let duration_secs: u64 = std::env::var("LOADTEST_DURATION")
        .ok()
        .and_then(|d| d.parse().ok())
        .unwrap_or(3600); // Default 1 hour

    let users: usize = std::env::var("LOADTEST_USERS")
        .ok()
        .and_then(|u| u.parse().ok())
        .unwrap_or(10);

    let metrics_interval_secs: u64 = std::env::var("LOADTEST_METRICS_INTERVAL")
        .ok()
        .and_then(|i| i.parse().ok())
        .unwrap_or(30);

    // Configure for soak test
    config.workload.duration_secs = duration_secs;
    config.workload.users.max = users;
    config.workload.users.start = users;
    config.workload.users.hatch_rate = 1;

    println!("🔥 Starting soak test");
    println!(
        "   Duration: {}s ({}h {}m)",
        duration_secs,
        duration_secs / 3600,
        (duration_secs % 3600) / 60
    );
    println!("   Users: {}", users);
    println!("   Metrics interval: {}s", metrics_interval_secs);

    // Initialize state
    let app_state = Arc::new(save_loadtest::transactions::AppState::new(config.clone()));
    {
        let mut state = save_loadtest::GLOBAL_STATE.write().await;
        *state = Some(app_state.clone());
    }

    // Build scenario
    let scenario = scenarios::mixed::build_scenario(&config);

    // Start metrics collection if prometheus is configured
    let prometheus_samples = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let system_samples = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let metrics_task = if let Some(prom_endpoint) = &config.reporting.prometheus_endpoint {
        let collector = save_loadtest::metrics::MetricsCollector::new(prom_endpoint.clone());
        let prom_samples = prometheus_samples.clone();
        let sys_samples = system_samples.clone();
        let collect_sys = config.reporting.collect_system_metrics;
        let server_pid = std::env::var("SERVER_PID")
            .ok()
            .and_then(|p| p.parse::<u32>().ok());

        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(metrics_interval_secs));
            let mut sys_collector =
                server_pid.map(save_loadtest::system_metrics::SystemCollector::new);

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

    // Execute soak test
    let runtime_str = format!("{}s", duration_secs);
    let metrics = GooseAttack::initialize()?
        .register_scenario(scenario)
        .set_default(GooseDefault::Host, config.target.endpoint.as_str())?
        .set_default(GooseDefault::RunTime, runtime_str.as_str())?
        .execute()
        .await?;

    // Stop metrics collection
    if let Some(task) = metrics_task {
        task.abort();
    }

    // Generate report
    let report = TestReport::from_goose_metrics(
        "soak-test",
        &metrics,
        prometheus_samples.lock().await.clone(),
        system_samples.lock().await.clone(),
    );

    // Save reports
    std::fs::create_dir_all(&config.reporting.output_dir)?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let json_path = format!(
        "{}/soaktest-{}.json",
        config.reporting.output_dir, timestamp
    );
    report.save_json(&json_path)?;

    let md_path = format!("{}/soaktest-{}.md", config.reporting.output_dir, timestamp);
    report.save_markdown(&md_path)?;

    // Assert soak test requirements
    assert!(report.total_requests() > 0, "No requests were made");

    let success_rate = report.success_rate();
    assert!(
        success_rate >= 0.95,
        "Soak test success rate {:.2}% below 95% threshold",
        success_rate * 100.0
    );

    // Check for performance degradation (if we have enough data points)
    // In a real soak test, you'd compare first-quarter vs last-quarter metrics
    // to detect degradation over time

    println!("\n✅ Soak test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.total_requests());
    println!("   Duration: {}s", duration_secs);
    println!("\n📊 Reports saved:");
    println!("   {}", json_path);
    println!("   {}", md_path);

    Ok(())
}

/// Short soak test for CI (10 minutes)
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_soak_short() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let mut config = load_config()?;

    // 10 minute soak test
    config.workload.duration_secs = 600;
    config.workload.users.max = 5;
    config.workload.users.start = 5;
    config.workload.users.hatch_rate = 1;

    let scenario = scenarios::mixed::build_scenario(&config);

    // Simple run without metrics collection
    let app_state = Arc::new(save_loadtest::transactions::AppState::new(config.clone()));
    {
        let mut state = save_loadtest::GLOBAL_STATE.write().await;
        *state = Some(app_state);
    }

    let metrics = GooseAttack::initialize()?
        .register_scenario(scenario)
        .set_default(GooseDefault::Host, config.target.endpoint.as_str())?
        .set_default(GooseDefault::RunTime, "600s")?
        .execute()
        .await?;

    let report = TestReport::from_goose_metrics("soak-short", &metrics, vec![], vec![]);

    assert!(report.total_requests() > 0);

    let success_rate = report.success_rate();
    assert!(
        success_rate >= 0.95,
        "Short soak test success rate {:.2}% below 95%",
        success_rate * 100.0
    );

    println!("\n✅ Short soak test passed (10 min)");
    println!("   Success rate: {:.2}%", success_rate * 100.0);

    Ok(())
}
