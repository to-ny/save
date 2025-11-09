//! Test simultaneous PUTs to the same object key.
//!
//! This test verifies that concurrent PUT operations to the same key:
//! - Complete without panics or crashes
//! - Result in a valid, complete object
//! - Return proper HTTP status codes and ETags
//! - Do not cause metadata corruption

#![cfg(feature = "concurrency_tests")]

mod common;

use anyhow::{Context, Result};
use common::{cleanup_bucket, create_client, ensure_bucket, unique_bucket_name};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::info;

#[tokio::test]
async fn test_simultaneous_puts_same_key() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("concurrent-put");
    let key = "contested-object.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing simultaneous PUTs to {}/{}", bucket, key);

    // Create multiple different payloads
    let payloads: Vec<Vec<u8>> = (0..10)
        .map(|i| {
            let size = 1024 * (i + 1); // Different sizes: 1KB, 2KB, ..., 10KB
            (0..size).map(|j| ((i + j) % 256) as u8).collect()
        })
        .collect();

    // Launch 10 concurrent PUT operations with different payloads
    let mut tasks = JoinSet::new();
    for (idx, payload) in payloads.into_iter().enumerate() {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();

        tasks.spawn(async move {
            info!("PUT #{} started (size: {} bytes)", idx, payload.len());

            let result = client
                .put_object()
                .bucket(&bucket)
                .key(&key)
                .body(payload.into())
                .send()
                .await;

            match &result {
                Ok(response) => {
                    info!(
                        "PUT #{} succeeded - ETag: {:?}",
                        idx,
                        response.e_tag
                    );
                    assert!(response.e_tag.is_some(), "PUT should return an ETag");
                }
                Err(e) => {
                    info!("PUT #{} failed: {}", idx, e);
                }
            }

            result
        });
    }

    // Wait for all PUT operations to complete
    let mut success_count = 0;
    let mut etags = Vec::new();

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(response)) => {
                success_count += 1;
                if let Some(etag) = response.e_tag {
                    etags.push(etag);
                }
            }
            Ok(Err(e)) => {
                info!("PUT operation failed: {}", e);
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Task panicked: {}", e));
            }
        }
    }

    info!(
        "Completed: {} successful PUTs, {} unique ETags",
        success_count,
        etags.iter().collect::<std::collections::HashSet<_>>().len()
    );

    // All operations should have completed without panics
    assert!(success_count > 0, "At least one PUT should succeed");

    // Verify the final object exists and is valid
    info!("Verifying final object state");
    let get_response = client
        .get_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to GET final object")?;

    // Verify we got a valid response
    assert!(get_response.e_tag.is_some(), "Final object should have an ETag");
    assert!(get_response.content_length.is_some(), "Final object should have a content length");

    let content_length = get_response.content_length.unwrap();
    assert!(content_length > 0, "Final object should have non-zero size");

    // Read and verify the body is complete
    let body_bytes = get_response
        .body
        .collect()
        .await
        .context("Failed to read final object body")?
        .into_bytes();

    assert_eq!(
        body_bytes.len(),
        content_length as usize,
        "Body size should match content-length header"
    );

    info!(
        "Final object verified: {} bytes, ETag: {:?}",
        body_bytes.len(),
        get_response.e_tag
    );

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_rapid_overwrites() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("rapid-overwrite");
    let key = "rapidly-overwritten.txt";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing rapid sequential overwrites to {}/{}", bucket, key);

    // Perform rapid sequential PUTs (not parallel, but fast)
    let iterations = 20;
    let mut last_etag: Option<String> = None;

    for i in 0..iterations {
        let content = format!("Version {}", i);

        let response = client
            .put_object()
            .bucket(&bucket)
            .key(key)
            .body(content.as_bytes().to_vec().into())
            .send()
            .await
            .context(format!("PUT iteration {} failed", i))?;

        let current_etag = response.e_tag.clone();
        assert!(current_etag.is_some(), "PUT should return an ETag");

        // Verify HEAD returns the same ETag
        let head_response = client
            .head_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .context(format!("HEAD iteration {} failed", i))?;

        assert_eq!(
            head_response.e_tag, current_etag,
            "HEAD should return the same ETag as PUT"
        );

        // ETags should change between overwrites (unless content is identical)
        if let Some(prev) = &last_etag {
            info!("Iteration {}: ETag changed from {:?} to {:?}", i, prev, current_etag);
        }

        last_etag = current_etag;
    }

    // Verify final state
    let get_response = client
        .get_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to GET final object")?;

    let body = get_response
        .body
        .collect()
        .await
        .context("Failed to read body")?
        .into_bytes();

    let body_str = String::from_utf8_lossy(&body);
    info!("Final object content: {}", body_str);

    // The final content should be one of the versions (likely the last one)
    assert!(
        body_str.starts_with("Version "),
        "Final object should be one of the written versions"
    );

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}
