#![cfg_attr(not(feature = "load_tests"), allow(unused_imports))]

use save_loadtest::{load_config, run_benchmark};

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_mixed_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let report = run_benchmark("mixed", &config).await?;

    assert!(
        report.metrics.total_requests > 0,
        "No requests were made during the test"
    );

    let success_rate =
        report.metrics.successful_requests as f64 / report.metrics.total_requests as f64;
    assert!(
        success_rate >= 0.95,
        "Success rate {:.2}% below 95% threshold",
        success_rate * 100.0
    );

    let p95 = report.metrics.end_to_end_latency.p95_ms;
    assert!(p95 < 1000, "P95 latency {}ms exceeds 1000ms threshold", p95);

    println!("\n✅ Mixed workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.metrics.total_requests);
    println!("   P95 latency: {}ms", p95);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_read_heavy_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let report = run_benchmark("read-heavy", &config).await?;

    assert!(report.metrics.total_requests > 0);

    let success_rate =
        report.metrics.successful_requests as f64 / report.metrics.total_requests as f64;
    assert!(
        success_rate >= 0.98,
        "Read-heavy success rate {:.2}% below 98% threshold",
        success_rate * 100.0
    );

    println!("\n✅ Read-heavy workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.metrics.total_requests);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_write_heavy_workload() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    let report = run_benchmark("write-heavy", &config).await?;

    assert!(report.metrics.total_requests > 0);

    let success_rate =
        report.metrics.successful_requests as f64 / report.metrics.total_requests as f64;
    assert!(
        success_rate >= 0.95,
        "Write-heavy success rate {:.2}% below 95% threshold",
        success_rate * 100.0
    );

    println!("\n✅ Write-heavy workload test passed");
    println!("   Success rate: {:.2}%", success_rate * 100.0);
    println!("   Total requests: {}", report.metrics.total_requests);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_quick_smoke() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let mut config = load_config()?;
    config.workload.duration_secs = 5;
    config.workload.users.max = 2;
    config.workload.users.start = 1;

    let report = run_benchmark("mixed", &config).await?;

    assert!(report.metrics.total_requests > 0, "No requests were made");

    println!("\n✅ Quick smoke test passed");
    println!("   Total requests: {}", report.metrics.total_requests);

    Ok(())
}
