//! Test concurrent GET operations while PUT is in progress.
//!
//! This test verifies that concurrent GET operations while PUT is happening:
//! - Either succeed with the old version or get a 404 (if object didn't exist)
//! - Eventually see the new version after PUT completes
//! - Don't get partial/corrupted data
//! - Don't cause panics or crashes

#![cfg(feature = "concurrency_tests")]

mod common;

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use common::{cleanup_bucket, create_client, ensure_bucket, unique_bucket_name};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tracing::info;

#[tokio::test]
async fn test_concurrent_gets_during_put() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("concurrent-get-put");
    let key = "changing-object.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing concurrent GETs during PUT to {}/{}", bucket, key);

    // First, upload an initial version
    let initial_content = b"Initial version of the object";
    client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(initial_content.to_vec().into())
        .send()
        .await
        .context("Failed to PUT initial object")?;

    info!("Initial object uploaded");

    // Prepare a larger object to upload (to increase the window for concurrent GETs)
    let new_content: Vec<u8> = (0..1024 * 100) // 100KB
        .map(|i| (i % 256) as u8)
        .collect();
    let new_content = Arc::new(new_content);

    // Launch concurrent GET operations
    let mut tasks = JoinSet::new();

    // Start GET operations
    for i in 0..10 {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();

        tasks.spawn(async move {
            sleep(Duration::from_millis(i * 10)).await; // Stagger slightly

            info!("GET #{} starting", i);
            let result = client.get_object().bucket(&bucket).key(&key).send().await;

            match result {
                Ok(response) => {
                    let etag = response.e_tag().unwrap_or("none").to_string();
                    let content_length = response.content_length.unwrap_or(0);
                    info!(
                        "GET #{} succeeded - {} bytes, ETag: {}",
                        i, content_length, etag
                    );

                    // Read the body to ensure it's not corrupted
                    let body = response.body.collect().await.ok()?;
                    let bytes = body.into_bytes();

                    // Verify size matches content-length
                    if bytes.len() != content_length as usize {
                        info!(
                            "GET #{} - SIZE MISMATCH: got {} bytes, expected {}",
                            i,
                            bytes.len(),
                            content_length
                        );
                        return Some((false, bytes.len(), content_length as usize));
                    }

                    // Verify ETag matches actual content hash
                    let actual_hash = format!("\"{:x}\"", Sha256::digest(&bytes));
                    if etag != actual_hash {
                        info!(
                            "GET #{} - ETAG MISMATCH: got {}, actual {}",
                            i, etag, actual_hash
                        );
                        return Some((false, bytes.len(), content_length as usize));
                    }

                    Some((true, bytes.len(), content_length as usize))
                }
                Err(e) => {
                    info!("GET #{} failed: {}", i, e);
                    None
                }
            }
        });
    }

    // Start a PUT operation after a small delay
    sleep(Duration::from_millis(25)).await;
    let put_client = Arc::clone(&client);
    let put_bucket = bucket.clone();
    let put_content = Arc::clone(&new_content);

    let put_task = tokio::spawn(async move {
        info!("PUT starting (new version, {} bytes)", put_content.len());
        put_client
            .put_object()
            .bucket(&put_bucket)
            .key(key)
            .body(ByteStream::from((*put_content).clone()))
            .send()
            .await
    });

    // Wait for all GET operations to complete
    let mut get_success = 0;
    let mut size_mismatches = 0;

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Some((matched, _, _))) => {
                get_success += 1;
                if !matched {
                    size_mismatches += 1;
                }
            }
            Ok(None) => {
                // GET failed, which is acceptable
            }
            Err(e) => {
                return Err(anyhow::anyhow!("GET task panicked: {}", e));
            }
        }
    }

    // Wait for PUT to complete
    let put_result = put_task.await.context("PUT task panicked")??;
    info!("PUT completed - ETag: {:?}", put_result.e_tag);

    // TODO Flaky test - Fails sometimes on this assertion
    // Verify no size mismatches (no partial reads)
    assert_eq!(
        size_mismatches, 0,
        "No GET should receive partial/corrupted data"
    );

    info!("All {} GETs succeeded without corruption", get_success);

    // Verify final state - should be the new version
    let final_response = client
        .get_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to GET final object")?;

    let final_etag = final_response.e_tag().unwrap_or("none").to_string();
    let final_body = final_response
        .body
        .collect()
        .await
        .context("Failed to read final body")?
        .into_bytes();

    assert_eq!(
        final_body.len(),
        new_content.len(),
        "Final object should be the new version"
    );
    assert_eq!(
        final_body.as_ref(),
        new_content.as_ref(),
        "Final object content should match new version"
    );

    let final_actual_hash = format!("\"{:x}\"", Sha256::digest(&final_body));
    assert_eq!(
        final_etag, final_actual_hash,
        "Final ETag must match actual content hash"
    );

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_get_nonexistent_during_put() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("get-during-create");
    let key = "new-object.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing GETs during initial PUT to {}/{}", bucket, key);

    // Object doesn't exist initially
    // Launch concurrent GET operations
    let mut tasks = JoinSet::new();

    for i in 0..10 {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();

        tasks.spawn(async move {
            sleep(Duration::from_millis(i * 5)).await;

            info!("GET #{} for non-existent object", i);
            let result = client.get_object().bucket(&bucket).key(&key).send().await;

            match &result {
                Ok(_) => {
                    info!("GET #{} succeeded", i);
                    true
                }
                Err(e) => {
                    info!("GET #{} failed (expected): {}", i, e);
                    false
                }
            }
        });
    }

    // Start a PUT after a small delay
    sleep(Duration::from_millis(15)).await;
    let content = vec![42u8; 1024 * 50]; // 50KB

    let put_client = Arc::clone(&client);
    let put_bucket = bucket.clone();
    let put_key = key.to_string();

    let put_task = tokio::spawn(async move {
        info!("PUT starting (creating new object)");
        put_client
            .put_object()
            .bucket(&put_bucket)
            .key(&put_key)
            .body(content.into())
            .send()
            .await
    });

    // Collect results
    let mut success = 0;
    let mut not_found = 0;

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(true) => success += 1,
            Ok(false) => not_found += 1,
            Err(e) => {
                return Err(anyhow::anyhow!("Task panicked: {}", e));
            }
        }
    }

    // Wait for PUT
    put_task.await.context("PUT task panicked")??;

    info!(
        "Results: {} GETs succeeded, {} got NotFound",
        success, not_found
    );

    // Either result is acceptable:
    // - GETs fail with NotFound (object doesn't exist yet)
    // - GETs succeed (object was created by PUT)
    // The key is no panics or corruption

    // Verify final state
    let final_response = client
        .get_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Final GET should succeed")?;

    let final_body = final_response
        .body
        .collect()
        .await
        .context("Failed to read final body")?
        .into_bytes();

    assert_eq!(final_body.len(), 1024 * 50, "Final object should be 50KB");

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}
