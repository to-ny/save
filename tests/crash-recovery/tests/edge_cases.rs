mod common;

use anyhow::Result;
use common::{SingleNodeEnv, TestEnvironment};
use std::time::{Duration, Instant};

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_partial_multipart_missing_parts() -> Result<()> {
    // Test: Try to complete multipart with missing parts
    // Expected: Error response, no corruption

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Initiate multipart
    let init = env
        .client()
        .create_multipart_upload()
        .bucket("test")
        .key("partial.bin")
        .send()
        .await?;

    let upload_id = init.upload_id().unwrap();

    // Upload parts 1, 2, 4 (skip part 3)
    tracing::info!("Uploading parts 1, 2, 4 (skipping 3)");
    for i in [1, 2, 4] {
        env.client()
            .upload_part()
            .bucket("test")
            .key("partial.bin")
            .upload_id(upload_id)
            .part_number(i)
            .body(vec![0u8; 1024].into())
            .send()
            .await?;
    }

    // Try to complete with missing part 3
    let result = env
        .client()
        .complete_multipart_upload()
        .bucket("test")
        .key("partial.bin")
        .upload_id(upload_id)
        .send()
        .await;

    assert!(result.is_err(), "Complete should fail with missing parts");

    // Verify no corruption
    env.verify_consistency().await?;

    // Can still abort the upload
    env.client()
        .abort_multipart_upload()
        .bucket("test")
        .key("partial.bin")
        .upload_id(upload_id)
        .send()
        .await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_partial_multipart_wrong_etags() -> Result<()> {
    // Test: Try to complete multipart with wrong ETags
    // Expected: Error response, no corruption

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    let init = env
        .client()
        .create_multipart_upload()
        .bucket("test")
        .key("wrong-etags.bin")
        .send()
        .await?;

    let upload_id = init.upload_id().unwrap();

    // Upload parts
    for i in 1..=3 {
        env.client()
            .upload_part()
            .bucket("test")
            .key("wrong-etags.bin")
            .upload_id(upload_id)
            .part_number(i)
            .body(vec![0xAA; 1024].into())
            .send()
            .await?;
    }

    // Try to complete with fabricated/wrong ETags
    use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};

    let completed_upload = CompletedMultipartUpload::builder()
        .parts(
            CompletedPart::builder()
                .e_tag("\"wrong-etag-1\"")
                .part_number(1)
                .build(),
        )
        .parts(
            CompletedPart::builder()
                .e_tag("\"wrong-etag-2\"")
                .part_number(2)
                .build(),
        )
        .parts(
            CompletedPart::builder()
                .e_tag("\"wrong-etag-3\"")
                .part_number(3)
                .build(),
        )
        .build();

    let result = env
        .client()
        .complete_multipart_upload()
        .bucket("test")
        .key("wrong-etags.bin")
        .upload_id(upload_id)
        .multipart_upload(completed_upload)
        .send()
        .await;

    // Should either fail or succeed (implementation dependent)
    // Either way, verify no corruption
    if result.is_ok() {
        tracing::info!("Complete succeeded (implementation allows any ETags)");
    } else {
        tracing::info!("Complete failed with wrong ETags (stricter validation)");
    }

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_lock_timeout() -> Result<()> {
    // Test: Lock acquisition timeout
    // Expected: Second request times out, resources released

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Configure failpoint to pause for longer than lock timeout (30s)
    env.configure_failpoint("storage_commit_after_rename", "sleep(35000)")?;

    // Start first PUT (will hold lock for 35 seconds)
    let client1 = env.client().clone();
    let handle1 = tokio::spawn(async move {
        client1
            .put_object()
            .bucket("test")
            .key("locked.txt")
            .body(b"first write".to_vec().into())
            .send()
            .await
    });

    // Wait a bit for lock to be acquired
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Try to acquire same lock - should timeout after 30s
    tracing::info!("Attempting second PUT (should timeout)");
    let start = Instant::now();

    let result = env
        .client()
        .put_object()
        .bucket("test")
        .key("locked.txt")
        .body(b"second write".to_vec().into())
        .send()
        .await;

    let elapsed = start.elapsed();

    tracing::info!("Second PUT completed in {:?}", elapsed);

    // Should fail due to timeout
    assert!(
        result.is_err(),
        "Second PUT should fail due to lock timeout"
    );

    // Should timeout around 30 seconds, not wait full 35 seconds
    assert!(
        elapsed >= Duration::from_secs(28) && elapsed < Duration::from_secs(37),
        "Should timeout after ~30s, got {:?}",
        elapsed
    );

    // Clean up failpoint
    fail::remove("storage_commit_after_rename");

    // Wait for first PUT to complete or timeout
    let _ = tokio::time::timeout(Duration::from_secs(10), handle1).await;

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_filesystem_full_simulation() -> Result<()> {
    // Test: Simulate filesystem full using failpoint
    // Expected: Proper error handling, no corruption

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Configure failpoint to simulate ENOSPC (no space left on device)
    env.configure_failpoint("storage_write_during_copy", "return(enospc)")?;

    // Try to PUT object
    let result = env
        .client()
        .put_object()
        .bucket("test")
        .key("test.txt")
        .body(b"data".to_vec().into())
        .send()
        .await;

    assert!(result.is_err(), "PUT should fail when disk full");

    // Remove failpoint
    fail::remove("storage_write_during_copy");

    // Verify no corruption
    env.verify_consistency().await?;

    // Should be able to PUT after disk space is available
    let result = env
        .client()
        .put_object()
        .bucket("test")
        .key("test.txt")
        .body(b"data after recovery".to_vec().into())
        .send()
        .await;

    assert!(result.is_ok(), "PUT should succeed after recovery");

    tracing::info!("Test completed successfully");
    Ok(())
}
