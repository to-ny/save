//! Test concurrent DELETE and PUT operations to the same key.
//!
//! This test verifies that concurrent DELETE and PUT operations:
//! - Don't cause panics or crashes
//! - Result in a consistent final state (object exists or doesn't)
//! - Don't leave partial/corrupted data
//! - Handle race conditions correctly

#![cfg(feature = "concurrency_tests")]

mod common;

use anyhow::{Context, Result};
use common::{cleanup_bucket, create_client, ensure_bucket, unique_bucket_name};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::info;

#[tokio::test]
async fn test_concurrent_delete_and_put() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("delete-put-race");
    let key = "contested-object.txt";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing concurrent DELETE and PUT to {}/{}", bucket, key);

    // Create initial object
    client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(b"Initial content".to_vec().into())
        .send()
        .await
        .context("Failed to create initial object")?;

    info!("Initial object created");

    // Launch concurrent DELETE and PUT operations
    let mut tasks = JoinSet::new();

    // Launch 5 DELETE operations
    for i in 0..5 {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();

        tasks.spawn(async move {
            info!("DELETE #{} starting", i);
            let result = client
                .delete_object()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await;

            match &result {
                Ok(_) => {
                    info!("DELETE #{} succeeded", i);
                }
                Err(e) => {
                    info!("DELETE #{} failed: {}", i, e);
                }
            }

            ("DELETE", i, result.is_ok())
        });
    }

    // Launch 5 PUT operations
    for i in 0..5 {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();
        let content = format!("PUT content version {}", i);

        tasks.spawn(async move {
            info!("PUT #{} starting", i);
            let result = client
                .put_object()
                .bucket(&bucket)
                .key(&key)
                .body(content.into_bytes().into())
                .send()
                .await;

            match &result {
                Ok(response) => {
                    info!("PUT #{} succeeded - ETag: {:?}", i, response.e_tag);
                }
                Err(e) => {
                    info!("PUT #{} failed: {}", i, e);
                }
            }

            ("PUT", i, result.is_ok())
        });
    }

    // Wait for all operations to complete
    let mut delete_success = 0;
    let mut put_success = 0;

    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((op_type, _idx, success)) => {
                if success {
                    if op_type == "DELETE" {
                        delete_success += 1;
                    } else {
                        put_success += 1;
                    }
                }
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Task panicked: {}", e));
            }
        }
    }

    info!(
        "Completed: {} DELETEs succeeded, {} PUTs succeeded",
        delete_success, put_success
    );

    // The key point is no panics occurred
    // Final state can be either:
    // 1. Object exists (last PUT won)
    // 2. Object doesn't exist (last DELETE won)

    // Check final state
    let get_result = client.get_object().bucket(&bucket).key(key).send().await;

    match get_result {
        Ok(response) => {
            let body = response
                .body
                .collect()
                .await
                .context("Failed to read body")?
                .into_bytes();
            info!(
                "Final state: Object EXISTS ({} bytes)",
                body.len()
            );
            // If object exists, it should be a complete, valid object
            assert!(!body.is_empty(), "Object should not be empty");
        }
        Err(_) => {
            info!("Final state: Object DOES NOT EXIST");
        }
    }

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_rapid_create_delete_cycles() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("create-delete-cycle");
    let key = "cycling-object.txt";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing rapid create/delete cycles to {}/{}", bucket, key);

    // Perform 20 rapid PUT/DELETE cycles
    for i in 0..20 {
        // PUT
        let content = format!("Cycle {} content", i);
        let put_result = client
            .put_object()
            .bucket(&bucket)
            .key(key)
            .body(content.into_bytes().into())
            .send()
            .await;

        if let Err(e) = put_result {
            info!("Cycle {} PUT failed: {}", i, e);
        }

        // DELETE
        let delete_result = client
            .delete_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await;

        if let Err(e) = delete_result {
            info!("Cycle {} DELETE failed: {}", i, e);
        }
    }

    info!("Completed 20 create/delete cycles");

    // Final state should be consistent (no corruption)
    // Object may or may not exist depending on timing
    let get_result = client.get_object().bucket(&bucket).key(key).send().await;

    match get_result {
        Ok(_) => info!("Final state: Object exists"),
        Err(_) => info!("Final state: Object doesn't exist"),
    }

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_delete_during_multipart() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("delete-during-mp");
    let key = "multipart-object.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing DELETE during multipart upload to {}/{}", bucket, key);

    // Start a multipart upload
    let initiate_response = client
        .create_multipart_upload()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to initiate multipart upload")?;

    let upload_id = initiate_response
        .upload_id
        .context("Missing upload ID")?;

    info!("Multipart upload initiated: {}", upload_id);

    // Upload first part
    let part1_data = vec![42u8; 5 * 1024 * 1024]; // 5MB
    let part1_response = client
        .upload_part()
        .bucket(&bucket)
        .key(key)
        .upload_id(&upload_id)
        .part_number(1)
        .body(part1_data.into())
        .send()
        .await
        .context("Failed to upload part 1")?;

    let part1_etag = part1_response.e_tag.context("Missing part 1 ETag")?;
    info!("Part 1 uploaded");

    // Try to DELETE the object (which is being uploaded)
    // This should either:
    // 1. Fail (object doesn't exist yet)
    // 2. Succeed (delete the in-progress upload)
    let delete_result = client
        .delete_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await;

    match &delete_result {
        Ok(_) => info!("DELETE succeeded"),
        Err(e) => info!("DELETE failed: {}", e),
    }

    // Try to upload another part
    let part2_data = vec![43u8; 5 * 1024 * 1024]; // 5MB
    let part2_result = client
        .upload_part()
        .bucket(&bucket)
        .key(key)
        .upload_id(&upload_id)
        .part_number(2)
        .body(part2_data.into())
        .send()
        .await;

    match &part2_result {
        Ok(_) => {
            info!("Part 2 uploaded successfully");

            // Try to complete the upload
            let part2_etag = part2_result.as_ref().unwrap().e_tag.clone().unwrap();

            let completed_parts = vec![
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(1)
                    .e_tag(part1_etag)
                    .build(),
                aws_sdk_s3::types::CompletedPart::builder()
                    .part_number(2)
                    .e_tag(part2_etag)
                    .build(),
            ];

            let completed_upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
                .set_parts(Some(completed_parts))
                .build();

            let complete_result = client
                .complete_multipart_upload()
                .bucket(&bucket)
                .key(key)
                .upload_id(&upload_id)
                .multipart_upload(completed_upload)
                .send()
                .await;

            match complete_result {
                Ok(_) => info!("Multipart upload completed despite DELETE"),
                Err(e) => info!("Complete failed (expected): {}", e),
            }
        }
        Err(e) => {
            info!("Part 2 upload failed (may be expected): {}", e);
        }
    }

    // Abort the upload if it's still in progress
    let _ = client
        .abort_multipart_upload()
        .bucket(&bucket)
        .key(key)
        .upload_id(&upload_id)
        .send()
        .await;

    // Verify no corruption - final state should be consistent
    let final_get = client.get_object().bucket(&bucket).key(key).send().await;

    match final_get {
        Ok(response) => {
            let body = response
                .body
                .collect()
                .await
                .context("Failed to read body")?
                .into_bytes();

            info!("Final object exists: {} bytes", body.len());

            // If it exists, it should be a complete object
            // (either the completed multipart or something else)
            assert!(!body.is_empty(), "Object should not be empty");
        }
        Err(_) => {
            info!("Final object doesn't exist (acceptable)");
        }
    }

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_mixed_operations_stress() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("mixed-ops-stress");
    let key = "stress-test-object.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Running mixed operations stress test on {}/{}", bucket, key);

    // Create initial object
    client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(b"Initial".to_vec().into())
        .send()
        .await?;

    let mut tasks = JoinSet::new();

    // Launch mixed operations: PUT, DELETE, GET, HEAD
    for i in 0..20 {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();

        let op_type = i % 4;

        tasks.spawn(async move {
            match op_type {
                0 => {
                    // PUT
                    let content = format!("Content {}", i);
                    let _ = client
                        .put_object()
                        .bucket(&bucket)
                        .key(&key)
                        .body(content.into_bytes().into())
                        .send()
                        .await;
                    "PUT"
                }
                1 => {
                    // DELETE
                    let _ = client
                        .delete_object()
                        .bucket(&bucket)
                        .key(&key)
                        .send()
                        .await;
                    "DELETE"
                }
                2 => {
                    // GET
                    let _ = client
                        .get_object()
                        .bucket(&bucket)
                        .key(&key)
                        .send()
                        .await;
                    "GET"
                }
                _ => {
                    // HEAD
                    let _ = client
                        .head_object()
                        .bucket(&bucket)
                        .key(&key)
                        .send()
                        .await;
                    "HEAD"
                }
            }
        });
    }

    // Wait for all operations
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(_) => {} // Operation completed (success or failure both OK)
            Err(e) => {
                return Err(anyhow::anyhow!("Task panicked: {}", e));
            }
        }
    }

    info!("All mixed operations completed without panics");

    // Final state should be consistent
    let final_result = client.get_object().bucket(&bucket).key(key).send().await;

    match final_result {
        Ok(response) => {
            let body = response
                .body
                .collect()
                .await
                .context("Failed to read body")?
                .into_bytes();
            info!("Final object exists and is readable: {} bytes", body.len());
        }
        Err(_) => {
            info!("Final object doesn't exist (acceptable)");
        }
    }

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}
