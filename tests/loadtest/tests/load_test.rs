#![cfg_attr(not(feature = "load_tests"), allow(unused_imports))]

use save_loadtest::{load_config, run_scenario_with_report, scenarios};

/// Test mixed workload scenario
///
/// Prerequisites:
/// - save-api server running at localhost:9000
/// - Test bucket "loadtest" created
///
/// Run with:
/// ```bash
/// cargo test --package save-loadtest --features load_tests test_mixed_workload
/// ```
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_mixed_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let scenario = scenarios::mixed::build_scenario(&config);

    let report = run_scenario_with_report(scenario, &config, "mixed").await?;

    // Assert basic SLO requirements
    assert!(
        report.total_requests() > 0,
        "No requests were made during the test"
    );

    // Success rate should be high (95%+)
    let success_rate = report.success_rate();
    assert!(
        success_rate >= 0.95,
        "Success rate {:.2}% below 95% threshold",
        success_rate * 100.0
    );

    // P95 latency should be reasonable (< 1 second for local testing)
    let p95 = report.execution.latency.p95_ms;
    assert!(
        p95 < 1000.0,
        "P95 latency {:.2}ms exceeds 1000ms threshold",
        p95
    );

    println!("\n✅ Mixed workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.total_requests());

    Ok(())
}

/// Test read-heavy workload scenario (80% reads)
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_read_heavy_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let scenario = scenarios::read_heavy::build_scenario(&config);

    let report = run_scenario_with_report(scenario, &config, "read-heavy").await?;

    assert!(report.total_requests() > 0);

    // Read-heavy workloads should have even higher success rates
    let success_rate = report.success_rate();
    assert!(
        success_rate >= 0.98,
        "Read-heavy success rate {:.2}% below 98% threshold",
        success_rate * 100.0
    );

    println!("\n✅ Read-heavy workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.total_requests());

    Ok(())
}

/// Test write-heavy workload scenario (70% writes)
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_write_heavy_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let scenario = scenarios::write_heavy::build_scenario(&config);

    let report = run_scenario_with_report(scenario, &config, "write-heavy").await?;

    assert!(report.total_requests() > 0);

    let success_rate = report.success_rate();
    assert!(
        success_rate >= 0.95,
        "Write-heavy success rate {:.2}% below 95% threshold",
        success_rate * 100.0
    );

    println!("\n✅ Write-heavy workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.total_requests());

    Ok(())
}

/// Test multipart upload scenario
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_multipart_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let scenario = scenarios::multipart::build_scenario(&config);

    let report = run_scenario_with_report(scenario, &config, "multipart").await?;

    assert!(report.total_requests() > 0);

    // Multipart uploads are more complex, allow slightly lower success rate
    let success_rate = report.success_rate();
    assert!(
        success_rate >= 0.90,
        "Multipart success rate {:.2}% below 90% threshold",
        success_rate * 100.0
    );

    println!("\n✅ Multipart workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.total_requests());

    Ok(())
}

/// Quick smoke test with minimal duration
#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_quick_smoke() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let mut config = load_config()?;

    // Override for quick test
    config.workload.duration_secs = 5;
    config.workload.users.max = 2;
    config.workload.users.start = 1;

    let scenario = scenarios::mixed::build_scenario(&config);
    let report = run_scenario_with_report(scenario, &config, "smoke").await?;

    assert!(report.total_requests() > 0, "No requests were made");

    // Just verify it runs, don't assert strict SLOs for smoke test
    println!("\n✅ Quick smoke test passed");
    println!("   Total requests: {}", report.total_requests());

    Ok(())
}
