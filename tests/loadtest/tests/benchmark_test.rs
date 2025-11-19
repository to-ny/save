#![cfg_attr(not(feature = "load_tests"), allow(unused_imports))]

use save_loadtest::{load_config, run_benchmark};

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_smoke_benchmark() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let mut config = load_config()?;
    config.workload.duration_secs = 5;
    config.workload.users.max = 2;

    println!("Running smoke benchmark test...");
    let report = run_benchmark("mixed", &config).await?;

    assert!(
        report.metrics.total_requests > 0,
        "Should have some requests"
    );
    println!(
        "Smoke test completed: {} requests",
        report.metrics.total_requests
    );

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_mixed_benchmark() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    println!("Running mixed workload benchmark...");
    let report = run_benchmark("mixed", &config).await?;

    println!("\n=== Benchmark Results ===");
    println!("Total requests: {}", report.metrics.total_requests);
    println!("Requests/sec: {:.2}", report.metrics.requests_per_second);
    println!("Throughput: {:.2} MB/s", report.metrics.throughput_mbps);
    println!(
        "p50 latency: {}ms",
        report.metrics.end_to_end_latency.p50_ms
    );
    println!(
        "p95 latency: {}ms",
        report.metrics.end_to_end_latency.p95_ms
    );

    if let Some(storage_latency) = &report.metrics.server_storage_latency {
        println!("\nServer-side storage latency:");
        println!("  p50: {}ms", storage_latency.p50_ms);
        println!("  p95: {}ms", storage_latency.p95_ms);
    }

    if let Some(network_latency) = &report.metrics.network_latency {
        println!("\nNetwork transfer latency:");
        println!("  p50: {}ms", network_latency.p50_ms);
        println!("  p95: {}ms", network_latency.p95_ms);
    }

    assert!(report.metrics.successful_requests > 0);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_write_heavy_benchmark() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    println!("Running write-heavy workload benchmark...");
    let report = run_benchmark("write-heavy", &config).await?;

    println!("\n=== Write-Heavy Results ===");
    println!("Total requests: {}", report.metrics.total_requests);
    println!("Throughput: {:.2} MB/s", report.metrics.throughput_mbps);

    assert!(report.metrics.successful_requests > 0);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "load_tests")]
async fn test_read_heavy_benchmark() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt::try_init();

    let config = load_config()?;
    println!("Running read-heavy workload benchmark...");
    let report = run_benchmark("read-heavy", &config).await?;

    println!("\n=== Read-Heavy Results ===");
    println!("Total requests: {}", report.metrics.total_requests);
    println!("Throughput: {:.2} MB/s", report.metrics.throughput_mbps);

    assert!(report.metrics.successful_requests > 0);

    Ok(())
}
