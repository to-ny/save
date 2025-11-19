#![cfg_attr(not(feature = "load_tests"), allow(unused_imports))]

use save_loadtest::{load_config, run_benchmark, run_benchmark_with_metrics};

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_soak() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let mut config = load_config()?;

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

    let report = run_benchmark_with_metrics("mixed", &config, metrics_interval_secs).await?;

    assert!(report.metrics.total_requests > 0, "No requests were made");

    let success_rate =
        report.metrics.successful_requests as f64 / report.metrics.total_requests as f64;
    assert!(
        success_rate >= 0.95,
        "Soak test success rate {:.2}% below 95% threshold",
        success_rate * 100.0
    );

    println!("\n✅ Soak test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.metrics.total_requests);
    println!("   Duration: {}s", duration_secs);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_soak_short() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let mut config = load_config()?;
    config.workload.duration_secs = 600; // 10 minutes
    config.workload.users.max = 5;
    config.workload.users.start = 5;
    config.workload.users.hatch_rate = 1;

    println!("🔥 Starting short soak test (10 minutes)");

    let report = run_benchmark("mixed", &config).await?;

    assert!(report.metrics.total_requests > 0);

    let success_rate =
        report.metrics.successful_requests as f64 / report.metrics.total_requests as f64;
    assert!(
        success_rate >= 0.95,
        "Short soak test success rate {:.2}% below 95%",
        success_rate * 100.0
    );

    println!("\n✅ Short soak test passed (10 min)");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.metrics.total_requests);

    Ok(())
}
