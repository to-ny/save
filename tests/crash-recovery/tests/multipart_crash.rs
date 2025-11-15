#![cfg_attr(not(feature = "crash_tests"), allow(unused_imports))]

mod common;

use anyhow::Result;
use common::{SingleNodeEnv, TestEnvironment};

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_multipart_crash_during_part_upload() -> Result<()> {
    // Test: Crash during part upload
    // Expected: Partial part file cleaned up by GC, multipart state intact

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Initiate multipart upload
    let init = env
        .client()
        .create_multipart_upload()
        .bucket("test")
        .key("multipart-crash.bin")
        .send()
        .await?;

    let upload_id = init.upload_id().unwrap();

    // Configure failpoint during part upload
    env.configure_failpoint("storage_write_during_copy", "pause")?;

    // Start part upload in background
    let client = env.client().clone();
    let upload_id_clone = upload_id.to_string();
    let _part_handle = tokio::spawn(async move {
        client
            .upload_part()
            .bucket("test")
            .key("multipart-crash.bin")
            .upload_id(&upload_id_clone)
            .part_number(1)
            .body(vec![0u8; 5 * 1024 * 1024].into())
            .send()
            .await
    });

    // Wait for failpoint
    env.wait_for_failpoint("storage_write_during_copy").await?;

    // CRASH!
    tracing::info!("Crashing during part upload");
    env.crash_at_failpoint("storage_write_during_copy").await?;

    // Restart
    tracing::info!("Restarting server");
    env.restart().await?;

    // Multipart upload state should still exist
    let list_uploads = env
        .client()
        .list_multipart_uploads()
        .bucket("test")
        .send()
        .await?;

    assert!(
        !list_uploads.uploads().is_empty(),
        "Multipart upload state should still exist"
    );

    // Can abort the upload
    env.client()
        .abort_multipart_upload()
        .bucket("test")
        .key("multipart-crash.bin")
        .upload_id(upload_id)
        .send()
        .await?;

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_multipart_crash_during_complete() -> Result<()> {
    // CRITICAL TEST: Crash during complete/assembly
    // Expected: Either multipart state remains or object is complete, no corruption

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Initiate multipart upload
    let init = env
        .client()
        .create_multipart_upload()
        .bucket("test")
        .key("multipart-complete-crash.bin")
        .send()
        .await?;

    let upload_id = init.upload_id().unwrap().to_string();

    // Upload parts successfully
    tracing::info!("Uploading parts");
    for i in 1..=3 {
        env.client()
            .upload_part()
            .bucket("test")
            .key("multipart-complete-crash.bin")
            .upload_id(&upload_id)
            .part_number(i)
            .body(vec![0xAA; 5 * 1024 * 1024].into())
            .send()
            .await?;
    }

    // Configure crash during complete (after storage commit, before metadata)
    env.configure_failpoint("storage_commit_after_fsync", "pause")?;

    // Start complete in background
    let client = env.client().clone();
    let upload_id_clone = upload_id.clone();
    let _complete_handle = tokio::spawn(async move {
        client
            .complete_multipart_upload()
            .bucket("test")
            .key("multipart-complete-crash.bin")
            .upload_id(&upload_id_clone)
            .send()
            .await
    });

    // Wait for failpoint
    env.wait_for_failpoint("storage_commit_after_fsync").await?;

    // CRASH!
    tracing::info!("Crashing during multipart complete");
    env.crash_at_failpoint("storage_commit_after_fsync").await?;

    // Restart
    tracing::info!("Restarting server");
    env.restart().await?;

    // Check state: either multipart still in progress or object complete
    let get_result = env
        .client()
        .get_object()
        .bucket("test")
        .key("multipart-complete-crash.bin")
        .send()
        .await;

    if get_result.is_err() {
        // Object not visible, multipart state should still exist
        tracing::info!("Object not visible, checking multipart state");
        let _list_uploads = env
            .client()
            .list_multipart_uploads()
            .bucket("test")
            .send()
            .await?;

        // Can retry complete or abort
        env.client()
            .abort_multipart_upload()
            .bucket("test")
            .key("multipart-complete-crash.bin")
            .upload_id(&upload_id)
            .send()
            .await?;
    } else {
        // Object is complete, verify it
        tracing::info!("Object completed despite crash");
        let obj = get_result.unwrap();
        let body = obj.body.collect().await?.into_bytes();
        assert_eq!(body.len(), 15 * 1024 * 1024, "Object should be complete");
    }

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_multipart_crash_after_complete() -> Result<()> {
    // Test: Crash after multipart upload fully completes
    // Expected: Object fully recoverable

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = SingleNodeEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Complete multipart upload normally
    let init = env
        .client()
        .create_multipart_upload()
        .bucket("test")
        .key("multipart-complete.bin")
        .send()
        .await?;

    let upload_id = init.upload_id().unwrap();

    for i in 1..=3 {
        env.client()
            .upload_part()
            .bucket("test")
            .key("multipart-complete.bin")
            .upload_id(upload_id)
            .part_number(i)
            .body(vec![0xBB; 5 * 1024 * 1024].into())
            .send()
            .await?;
    }

    env.client()
        .complete_multipart_upload()
        .bucket("test")
        .key("multipart-complete.bin")
        .upload_id(upload_id)
        .send()
        .await?;

    // Verify object exists
    let get = env
        .client()
        .get_object()
        .bucket("test")
        .key("multipart-complete.bin")
        .send()
        .await?;

    let body = get.body.collect().await?.into_bytes();
    assert_eq!(body.len(), 15 * 1024 * 1024);

    // Now crash
    tracing::info!("Crashing after complete multipart");
    env.crash_at_failpoint("").await?;

    // Restart
    tracing::info!("Restarting server");
    env.restart().await?;

    // Object should be fully recoverable
    let get = env
        .client()
        .get_object()
        .bucket("test")
        .key("multipart-complete.bin")
        .send()
        .await?;

    let body = get.body.collect().await?.into_bytes();
    assert_eq!(body.len(), 15 * 1024 * 1024);

    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}
