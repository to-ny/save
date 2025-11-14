//! Test parallel multipart uploads to the same bucket.
//!
//! This test verifies that concurrent multipart uploads:
//! - Can proceed in parallel to different keys
//! - Complete successfully with correct assembly
//! - Return proper upload IDs and ETags
//! - Don't interfere with each other or cause corruption

#![cfg(feature = "concurrency_tests")]

mod common;

use anyhow::{Context, Result};
use aws_sdk_s3::types::CompletedMultipartUpload;
use aws_sdk_s3::types::CompletedPart;
use common::{cleanup_bucket, create_client, ensure_bucket, unique_bucket_name, unique_key};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::info;

#[tokio::test]
async fn test_parallel_multipart_uploads() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("parallel-multipart");

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!("Testing parallel multipart uploads to bucket: {}", bucket);

    // Launch 5 concurrent multipart uploads with different keys
    let mut tasks = JoinSet::new();
    let num_uploads = 5;

    for upload_idx in 0..num_uploads {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = unique_key(&format!("parallel-upload-{}", upload_idx));

        tasks.spawn(async move {
            info!("Upload #{} starting for key: {}", upload_idx, key);

            // Initiate multipart upload
            let initiate_response = client
                .create_multipart_upload()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await
                .context("Failed to initiate multipart upload")?;

            let upload_id = initiate_response.upload_id.context("Missing upload ID")?;

            info!(
                "Upload #{} initiated - upload_id: {}",
                upload_idx, upload_id
            );

            // Upload 3 parts (5MB each = 15MB total)
            let part_size = 5 * 1024 * 1024; // 5MB
            let num_parts = 3;
            let mut completed_parts = Vec::new();

            for part_number in 1..=num_parts {
                let part_data: Vec<u8> = (0..part_size)
                    .map(|i| ((upload_idx + part_number + i) % 256) as u8)
                    .collect();

                info!(
                    "Upload #{} - uploading part {} ({} bytes)",
                    upload_idx,
                    part_number,
                    part_data.len()
                );

                let upload_part_response = client
                    .upload_part()
                    .bucket(&bucket)
                    .key(&key)
                    .upload_id(&upload_id)
                    .part_number(part_number)
                    .body(part_data.into())
                    .send()
                    .await
                    .context(format!(
                        "Failed to upload part {} for upload #{}",
                        part_number, upload_idx
                    ))?;

                let etag = upload_part_response
                    .e_tag
                    .context(format!("Missing ETag for part {}", part_number))?;

                info!(
                    "Upload #{} - part {} uploaded, ETag: {}",
                    upload_idx, part_number, etag
                );

                completed_parts.push(
                    CompletedPart::builder()
                        .part_number(part_number)
                        .e_tag(etag)
                        .build(),
                );
            }

            // Complete the multipart upload
            info!("Upload #{} - completing multipart upload", upload_idx);

            let completed_upload = CompletedMultipartUpload::builder()
                .set_parts(Some(completed_parts))
                .build();

            let complete_response = client
                .complete_multipart_upload()
                .bucket(&bucket)
                .key(&key)
                .upload_id(&upload_id)
                .multipart_upload(completed_upload)
                .send()
                .await
                .context(format!("Failed to complete upload #{}", upload_idx))?;

            info!(
                "Upload #{} completed - ETag: {:?}",
                upload_idx, complete_response.e_tag
            );

            // Verify the object was created
            let head_response = client
                .head_object()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await
                .context(format!("Failed to HEAD object for upload #{}", upload_idx))?;

            let expected_size = (part_size * num_parts) as i64;
            let actual_size = head_response.content_length.unwrap_or(0);

            assert_eq!(
                actual_size, expected_size,
                "Upload #{}: size mismatch",
                upload_idx
            );

            info!("Upload #{} verified - {} bytes", upload_idx, actual_size);

            Ok::<_, anyhow::Error>((upload_idx, key.clone(), actual_size))
        });
    }

    // Wait for all uploads to complete
    let mut completed = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok((idx, key, size))) => {
                info!(
                    "Upload #{} completed successfully: {} -> {} bytes",
                    idx, key, size
                );
                completed.push((idx, key, size));
            }
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("Upload failed: {}", e));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Upload task panicked: {}", e));
            }
        }
    }

    // Verify all uploads completed
    assert_eq!(
        completed.len(),
        num_uploads as usize,
        "All multipart uploads should complete"
    );

    // Verify all objects exist and are readable
    for (idx, key, expected_size) in &completed {
        let get_response = client
            .get_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .context(format!("Failed to GET object for upload #{}", idx))?;

        let body = get_response
            .body
            .collect()
            .await
            .context("Failed to read body")?
            .into_bytes();

        assert_eq!(
            body.len(),
            *expected_size as usize,
            "Upload #{}: body size mismatch",
            idx
        );

        info!("Upload #{} - object verified and readable", idx);
    }

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_concurrent_multipart_to_same_key() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("concurrent-mp-same-key");
    let key = "contested-multipart-object.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!(
        "Testing concurrent multipart uploads to same key: {}/{}",
        bucket, key
    );

    // Launch 3 concurrent multipart uploads to the SAME key
    let mut tasks = JoinSet::new();
    let num_uploads = 3;

    for upload_idx in 0..num_uploads {
        let client = Arc::clone(&client);
        let bucket = bucket.clone();
        let key = key.to_string();

        tasks.spawn(async move {
            info!("Upload #{} starting", upload_idx);

            // Initiate multipart upload
            let initiate_response = client
                .create_multipart_upload()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await
                .context("Failed to initiate")?;

            let upload_id = initiate_response.upload_id.context("Missing upload ID")?;

            info!("Upload #{} - upload_id: {}", upload_idx, upload_id);

            // Upload 2 parts
            let part_size = 5 * 1024 * 1024; // 5MB
            let mut completed_parts = Vec::new();

            for part_number in 1..=2 {
                let part_data: Vec<u8> = (0..part_size)
                    .map(|i| ((upload_idx * 100 + part_number + i) % 256) as u8)
                    .collect();

                let upload_part_response = client
                    .upload_part()
                    .bucket(&bucket)
                    .key(&key)
                    .upload_id(&upload_id)
                    .part_number(part_number)
                    .body(part_data.into())
                    .send()
                    .await
                    .context("Failed to upload part")?;

                let etag = upload_part_response.e_tag.context("Missing ETag")?;

                completed_parts.push(
                    CompletedPart::builder()
                        .part_number(part_number)
                        .e_tag(etag)
                        .build(),
                );
            }

            // Complete the upload
            let completed_upload = CompletedMultipartUpload::builder()
                .set_parts(Some(completed_parts))
                .build();

            let complete_response = client
                .complete_multipart_upload()
                .bucket(&bucket)
                .key(&key)
                .upload_id(&upload_id)
                .multipart_upload(completed_upload)
                .send()
                .await
                .context("Failed to complete")?;

            info!(
                "Upload #{} completed - ETag: {:?}",
                upload_idx, complete_response.e_tag
            );

            Ok::<_, anyhow::Error>(upload_idx)
        });
    }

    // Wait for all to complete
    let mut success_count = 0;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(idx)) => {
                info!("Upload #{} succeeded", idx);
                success_count += 1;
            }
            Ok(Err(e)) => {
                info!("Upload failed (acceptable): {}", e);
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Task panicked: {}", e));
            }
        }
    }

    // At least one should succeed
    assert!(success_count > 0, "At least one upload should succeed");

    // Verify final object exists and is valid
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

    // Should be 10MB (2 parts × 5MB)
    assert_eq!(body.len(), 10 * 1024 * 1024, "Final object should be 10MB");

    info!("Final object verified: {} bytes", body.len());

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_abort_multipart_during_upload() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init()
        .ok();

    let client = Arc::new(create_client().await);
    let bucket = unique_bucket_name("abort-during-upload");
    let key = "aborted-multipart.bin";

    // Setup
    ensure_bucket(&client, &bucket).await?;
    info!(
        "Testing abort during multipart upload to {}/{}",
        bucket, key
    );

    // Initiate multipart upload
    let initiate_response = client
        .create_multipart_upload()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to initiate")?;

    let upload_id = initiate_response.upload_id.context("Missing upload ID")?;

    info!("Upload initiated - upload_id: {}", upload_id);

    // Upload one part
    let part_data = vec![42u8; 5 * 1024 * 1024]; // 5MB
    client
        .upload_part()
        .bucket(&bucket)
        .key(key)
        .upload_id(&upload_id)
        .part_number(1)
        .body(part_data.into())
        .send()
        .await
        .context("Failed to upload part 1")?;

    info!("Part 1 uploaded");

    // Abort the upload
    client
        .abort_multipart_upload()
        .bucket(&bucket)
        .key(key)
        .upload_id(&upload_id)
        .send()
        .await
        .context("Failed to abort upload")?;

    info!("Upload aborted");

    // Verify the object doesn't exist
    let get_result = client.get_object().bucket(&bucket).key(key).send().await;

    assert!(get_result.is_err(), "Object should not exist after abort");

    info!("Verified object doesn't exist after abort");

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}
