#![cfg_attr(not(feature = "crash_tests"), allow(unused_imports))]

mod common;

use anyhow::Result;
use common::{ClusterEnv, TestEnvironment};

#[tokio::test]
#[cfg(feature = "crash_tests")]
async fn test_delete_crash_after_metadata() -> Result<()> {
    // Test: Crash after metadata deleted, before storage deleted
    // Expected: Object not visible, orphaned storage will be GC'd

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let mut env = ClusterEnv::setup().await?;

    env.client().create_bucket().bucket("test").send().await?;

    // Create object
    env.client()
        .put_object()
        .bucket("test")
        .key("delete-test.txt")
        .body(b"data to be deleted".to_vec().into())
        .send()
        .await?;

    // Verify object exists
    let get = env
        .client()
        .get_object()
        .bucket("test")
        .key("delete-test.txt")
        .send()
        .await?;

    let body = get.body.collect().await?.into_bytes();
    assert_eq!(body.as_ref(), b"data to be deleted");

    // Configure crash after metadata delete
    env.configure_failpoint("metadata_delete_after_write", "pause")
        .await?;

    // Start DELETE in background
    let client = env.client().clone();
    let _delete_handle = tokio::spawn(async move {
        client
            .delete_object()
            .bucket("test")
            .key("delete-test.txt")
            .send()
            .await
    });

    // Wait for failpoint
    env.wait_for_failpoint("metadata_delete_after_write")
        .await?;

    // CRASH!
    tracing::info!("Crashing after metadata delete");
    env.crash_at_failpoint("metadata_delete_after_write")
        .await?;

    // At this point:
    // - Metadata deleted
    // - Storage NOT deleted
    // - Orphaned storage file

    // Restart
    tracing::info!("Restarting server");
    env.restart().await?;

    // Object should NOT be visible (metadata deleted)
    let get_result = env
        .client()
        .get_object()
        .bucket("test")
        .key("delete-test.txt")
        .send()
        .await;

    assert!(
        get_result.is_err(),
        "Object should not be visible after metadata deleted"
    );

    // Verify consistency (orphaned storage will be detected)
    tracing::info!("Verifying consistency");
    env.verify_consistency().await?;

    tracing::info!("Test completed successfully");
    Ok(())
}
