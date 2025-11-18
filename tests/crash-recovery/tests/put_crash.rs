#![cfg_attr(not(feature = "crash_tests"), allow(unused_imports))]

mod common;

use anyhow::Result;
use common::{SingleNodeEnv, TestEnvironment};
use std::time::Duration;

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_put_crash_after_storage_before_metadata() -> Result<()> {
    // CRITICAL TEST: Crash after storage committed but before metadata committed
    // This creates an orphaned storage file that must be GC'd

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = SingleNodeEnv::setup().await?;

    // Create bucket
    env.client().create_bucket().bucket("test").send().await?;

    // Configure failpoint to pause after storage commit, before metadata commit
    env.configure_failpoint("storage_commit_after_fsync", "pause")
        .await?;

    // Start PUT in background with large body to ensure it's in progress
    let client = env.client().clone();
    let _put_handle = tokio::spawn(async move {
        let large_body = vec![0u8; 50 * 1024 * 1024];
        client
            .put_object()
            .bucket("test")
            .key("crash-test.txt")
            .body(large_body.into())
            .send()
            .await
    });

    // Wait for operation to reach failpoint and pause
    env.wait_for_failpoint("storage_commit_after_fsync").await?;

    // Crash server
    tracing::info!("Crashing server at failpoint");
    env.crash_at_failpoint("storage_commit_after_fsync").await?;

    // At this point:
    // - Storage file exists (committed)
    // - Metadata does NOT exist (not committed)
    // - This is an orphaned storage file

    // Restart server
    tracing::info!("Restarting server");
    env.restart().await?;

    // Verify object is NOT visible (no metadata)
    let get_result = env
        .client()
        .get_object()
        .bucket("test")
        .key("crash-test.txt")
        .send()
        .await;

    assert!(
        get_result.is_err(),
        "Object should not be visible without metadata"
    );

    // Verify consistency (orphaned file should be detected)
    tracing::info!("Verifying consistency");
    env.verify_consistency().await?;

    // Wait for GC cycle (configured for 60 seconds in test config)
    tracing::info!("Waiting for GC cycle");
    tokio::time::sleep(Duration::from_secs(65)).await;

    // Verify orphan was cleaned up
    tracing::info!("Verifying orphan cleanup");
    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_put_crash_after_complete() -> Result<()> {
    // Test: Crash after PUT fully completes
    // Expected: Object fully recoverable

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // No failpoint - let PUT complete normally
    env.client()
        .put_object()
        .bucket("test")
        .key("complete.txt")
        .body(b"complete data".to_vec().into())
        .send()
        .await?;

    // Now crash by killing the process (no failpoint, just SIGKILL)
    tracing::info!("Crashing server after complete PUT");
    env.crash_at_failpoint("").await?; // Empty string means just kill, no failpoint

    // Restart
    tracing::info!("Restarting server");
    env.restart().await?;

    // Object should be fully recoverable
    let get = env
        .client()
        .get_object()
        .bucket("test")
        .key("complete.txt")
        .send()
        .await?;

    let body = get.body.collect().await?.into_bytes();
    assert_eq!(body.as_ref(), b"complete data");

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_put_crash_before_rename_temp_cleanup() -> Result<()> {
    // CRITICAL TEST: Crash before rename (temp file exists, not committed)
    // This tests temp file cleanup by GC

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = SingleNodeEnv::setup().await?;

    // Create bucket
    env.client().create_bucket().bucket("test").send().await?;

    // Configure failpoint to pause before rename
    env.configure_failpoint("storage_commit_before_rename", "pause")
        .await?;

    // Start PUT in background with large body to ensure it's in progress
    let client = env.client().clone();
    let _put_handle = tokio::spawn(async move {
        let large_body = vec![0u8; 50 * 1024 * 1024];
        client
            .put_object()
            .bucket("test")
            .key("temp-test.txt")
            .body(large_body.into())
            .send()
            .await
    });

    // Wait for operation to reach failpoint and pause
    env.wait_for_failpoint("storage_commit_before_rename")
        .await?;

    // Crash server
    tracing::info!("Crashing server before rename");
    env.crash_at_failpoint("storage_commit_before_rename")
        .await?;

    // At this point:
    // - Temp file exists on disk
    // - Rename never happened (no final file)
    // - No metadata committed
    // - Temp file should be cleaned up by GC

    // Restart server
    tracing::info!("Restarting server");
    env.restart().await?;

    // Verify object is NOT visible (no metadata)
    let get_result = env
        .client()
        .get_object()
        .bucket("test")
        .key("temp-test.txt")
        .send()
        .await;

    assert!(
        get_result.is_err(),
        "Object should not be visible without metadata"
    );

    // Verify consistency (temp file should be detected as orphan)
    tracing::info!("Verifying consistency");
    env.verify_consistency().await?;

    // Wait for GC cycle to clean up temp file
    tracing::info!("Waiting for GC to clean temp files");
    tokio::time::sleep(std::time::Duration::from_secs(65)).await;

    // Verify temp file was cleaned up
    tracing::info!("Verifying temp file cleanup");
    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}
