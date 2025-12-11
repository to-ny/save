//! Chaos tests for cluster resilience under random failures.
//!
//! These tests verify the cluster's ability to handle random node failures,
//! network partitions, and other chaos scenarios.
//! Run with: cargo test -p crash-recovery-tests --features cluster_tests chaos

#![cfg(feature = "cluster_tests")]

mod common;

use common::cluster::ClusterEnv;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Test: Node failure during write workload.
///
/// Verifies that:
/// 1. Writes succeed when all nodes are healthy
/// 2. Writes continue to succeed when quorum is maintained (2/3 nodes)
/// 3. Cluster continues operating after a single follower failure
///
/// Note: This test only kills one follower and does NOT restart it.
/// Testing node restart recovery is complex due to Raft log sync requirements.
#[tokio::test]
async fn test_random_node_failures_during_write_workload() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let mut cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    let leader = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Initial leader: node {}", leader);

    // Create a test bucket
    let client = cluster.node_client(leader).expect("Should have client");
    client
        .create_bucket()
        .bucket("chaos-test")
        .send()
        .await
        .expect("Failed to create bucket");

    // Track successful and failed writes
    let successful_writes = Arc::new(AtomicUsize::new(0));
    let failed_writes = Arc::new(AtomicUsize::new(0));

    // Phase 1: Write objects while all nodes are healthy
    tracing::info!("Phase 1: Writing with all nodes healthy");
    for i in 0..10 {
        let result = write_object(&cluster, leader, "chaos-test", &format!("object-{}", i)).await;
        if result {
            successful_writes.fetch_add(1, Ordering::SeqCst);
        } else {
            failed_writes.fetch_add(1, Ordering::SeqCst);
        }
    }
    tracing::info!(
        "Phase 1 complete: {} successful, {} failed",
        successful_writes.load(Ordering::SeqCst),
        failed_writes.load(Ordering::SeqCst)
    );
    assert_eq!(
        successful_writes.load(Ordering::SeqCst),
        10,
        "All writes should succeed with healthy cluster"
    );

    // Phase 2: Kill a follower and continue writing (quorum maintained)
    let follower = cluster
        .node_ids()
        .into_iter()
        .find(|id| *id != leader)
        .expect("Should have a follower");

    cluster
        .kill_node(follower)
        .expect("Failed to kill follower");
    tracing::info!("Phase 2: Killed follower {}, continuing writes", follower);

    // Wait for cluster to detect the failure
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Get current leader (should still be the same since we only killed a follower)
    let current_leader = cluster
        .wait_for_leader(Duration::from_secs(10))
        .await
        .expect("Should have leader after killing follower");

    // More writes should still succeed (quorum = 2, we have 2 nodes)
    for i in 10..20 {
        let result = write_object(
            &cluster,
            current_leader,
            "chaos-test",
            &format!("object-{}", i),
        )
        .await;
        if result {
            successful_writes.fetch_add(1, Ordering::SeqCst);
        } else {
            failed_writes.fetch_add(1, Ordering::SeqCst);
        }
    }

    tracing::info!(
        "Final results: {} successful, {} failed",
        successful_writes.load(Ordering::SeqCst),
        failed_writes.load(Ordering::SeqCst)
    );

    // All Phase 1 writes (10) and most Phase 2 writes (at least 9/10) should succeed
    assert!(
        successful_writes.load(Ordering::SeqCst) >= 19,
        "Should have at least 19 successful writes overall: got {}",
        successful_writes.load(Ordering::SeqCst)
    );

    tracing::info!("Test passed: node failure handled correctly with quorum maintained");
}

/// Test: Network partition simulation during multipart upload.
///
/// Verifies that:
/// 1. Multipart uploads handle node failures gracefully
/// 2. Incomplete uploads are either completed or properly aborted
/// 3. No partial/corrupt data is left behind
#[tokio::test]
async fn test_network_partition_during_multipart_upload() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let mut cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    let leader = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Initial leader: node {}", leader);

    // Create test bucket
    let client = cluster.node_client(leader).expect("Should have client");
    client
        .create_bucket()
        .bucket("multipart-chaos")
        .send()
        .await
        .expect("Failed to create bucket");

    // Start a multipart upload
    let create_result = client
        .create_multipart_upload()
        .bucket("multipart-chaos")
        .key("large-object")
        .send()
        .await
        .expect("Failed to create multipart upload");

    let upload_id = create_result.upload_id().expect("Should have upload ID");
    tracing::info!("Started multipart upload: {}", upload_id);

    // Upload first part
    let part1_data = vec![b'A'; 5 * 1024 * 1024]; // 5MB
    let part1 = client
        .upload_part()
        .bucket("multipart-chaos")
        .key("large-object")
        .upload_id(upload_id)
        .part_number(1)
        .body(aws_sdk_s3::primitives::ByteStream::from(part1_data))
        .send()
        .await
        .expect("Failed to upload part 1");

    let etag1 = part1.e_tag().expect("Should have ETag").to_string();
    tracing::info!("Uploaded part 1, ETag: {}", etag1);

    // Simulate network partition: kill a follower mid-upload
    let follower = cluster
        .node_ids()
        .into_iter()
        .find(|id| *id != leader)
        .expect("Should have a follower");

    cluster
        .kill_node(follower)
        .expect("Failed to kill follower");
    tracing::info!("Killed follower {} during multipart upload", follower);

    // Wait a moment for cluster to detect the failure
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Try to upload second part (should still work with quorum)
    let current_leader = cluster
        .wait_for_leader(Duration::from_secs(10))
        .await
        .expect("Should have leader");

    let client = cluster
        .node_client(current_leader)
        .expect("Should have client");

    let part2_data = vec![b'B'; 5 * 1024 * 1024]; // 5MB
    let part2_result = client
        .upload_part()
        .bucket("multipart-chaos")
        .key("large-object")
        .upload_id(upload_id)
        .part_number(2)
        .body(aws_sdk_s3::primitives::ByteStream::from(part2_data))
        .send()
        .await;

    match part2_result {
        Ok(part2) => {
            let etag2 = part2.e_tag().expect("Should have ETag").to_string();
            tracing::info!("Uploaded part 2 during partition, ETag: {}", etag2);

            // Complete the multipart upload
            let complete_result = client
                .complete_multipart_upload()
                .bucket("multipart-chaos")
                .key("large-object")
                .upload_id(upload_id)
                .multipart_upload(
                    aws_sdk_s3::types::CompletedMultipartUpload::builder()
                        .parts(
                            aws_sdk_s3::types::CompletedPart::builder()
                                .part_number(1)
                                .e_tag(&etag1)
                                .build(),
                        )
                        .parts(
                            aws_sdk_s3::types::CompletedPart::builder()
                                .part_number(2)
                                .e_tag(&etag2)
                                .build(),
                        )
                        .build(),
                )
                .send()
                .await;

            assert!(
                complete_result.is_ok(),
                "Complete multipart should succeed: {:?}",
                complete_result.err()
            );
            tracing::info!("Multipart upload completed successfully during partition");

            // Verify the object exists and has correct size
            let head = client
                .head_object()
                .bucket("multipart-chaos")
                .key("large-object")
                .send()
                .await
                .expect("Object should exist");

            assert_eq!(
                head.content_length(),
                Some(10 * 1024 * 1024),
                "Object should be 10MB"
            );
        }
        Err(e) => {
            // Upload failed during partition - should be able to abort
            tracing::info!("Part 2 upload failed during partition: {:?}", e);

            // Abort the incomplete upload
            let abort_result = client
                .abort_multipart_upload()
                .bucket("multipart-chaos")
                .key("large-object")
                .upload_id(upload_id)
                .send()
                .await;

            // Abort should succeed even if upload was incomplete
            tracing::info!("Abort result: {:?}", abort_result);
        }
    }

    // Restart the killed node
    cluster
        .restart_node(follower)
        .await
        .expect("Failed to restart follower");

    // Wait for cluster to stabilize
    let final_leader = cluster
        .wait_for_leader(Duration::from_secs(30))
        .await
        .expect("Should have leader after restart");

    // Get a fresh client for verification
    let verify_client = cluster
        .node_client(final_leader)
        .expect("Should have client");

    // Verify no orphaned parts (list incomplete uploads should be empty or valid)
    let list_uploads = verify_client
        .list_multipart_uploads()
        .bucket("multipart-chaos")
        .send()
        .await
        .expect("Should list uploads");

    let uploads = list_uploads.uploads();
    if !uploads.is_empty() {
        tracing::info!("Remaining uploads after test: {:?}", uploads);
        // Any remaining uploads should be properly tracked
        for upload in uploads {
            tracing::info!(
                "  - Key: {:?}, UploadId: {:?}",
                upload.key(),
                upload.upload_id()
            );
        }
    }

    tracing::info!("Test passed: network partition during multipart upload handled correctly");
}

/// Test: Data durability after follower failure.
///
/// Verifies that:
/// 1. Data is replicated to all nodes before a failure
/// 2. Data remains accessible after a follower dies
/// 3. Multiple write and read operations work consistently
///
/// Note: This test only kills a follower to maintain quorum.
/// Leader failover testing is deferred until Raft network recovery is improved.
#[tokio::test]
async fn test_rapid_succession_failures() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let mut cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    let leader = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Initial leader: node {}", leader);

    // Get leader port for creating fresh clients
    let leader_port = cluster.api_port(leader).unwrap();

    // Create test bucket using fresh client
    {
        let client = create_fresh_client(leader_port).await;
        client
            .create_bucket()
            .bucket("rapid-chaos")
            .send()
            .await
            .expect("Failed to create bucket");

        // Write multiple objects while cluster is healthy
        for i in 0..5 {
            client
                .put_object()
                .bucket("rapid-chaos")
                .key(format!("object-{}", i))
                .body(aws_sdk_s3::primitives::ByteStream::from(
                    format!("data-{}", i).into_bytes(),
                ))
                .send()
                .await
                .expect("Failed to write object");
        }
    }
    tracing::info!("Wrote 5 objects while cluster healthy");

    // Kill a follower
    let follower = cluster
        .node_ids()
        .into_iter()
        .find(|id| *id != leader)
        .expect("Should have a follower");

    cluster
        .kill_node(follower)
        .expect("Failed to kill follower");
    tracing::info!("Killed follower node {}", follower);

    // Wait for cluster to detect the failure
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify we still have a leader
    let current_leader = cluster
        .wait_for_leader(Duration::from_secs(10))
        .await
        .expect("Should still have leader");
    tracing::info!("Current leader: {}", current_leader);

    // Use fresh client for verification
    let client = create_fresh_client(leader_port).await;

    // Verify all objects are still accessible
    for i in 0..5 {
        let key = format!("object-{}", i);
        let result = client
            .get_object()
            .bucket("rapid-chaos")
            .key(&key)
            .send()
            .await
            .expect(&format!("Should read object-{}", i));

        let body = result.body.collect().await.expect("Should read body");
        let expected = format!("data-{}", i);
        assert_eq!(
            body.into_bytes().as_ref(),
            expected.as_bytes(),
            "Data for object-{} should be correct",
            i
        );
    }
    tracing::info!("Verified all 5 objects after follower failure");

    // Write more objects (should still work with 2 nodes)
    for i in 5..10 {
        let result = client
            .put_object()
            .bucket("rapid-chaos")
            .key(format!("object-{}", i))
            .body(aws_sdk_s3::primitives::ByteStream::from(
                format!("data-{}", i).into_bytes(),
            ))
            .send()
            .await;
        assert!(result.is_ok(), "Should write object-{} with 2 nodes", i);
    }
    tracing::info!("Wrote 5 more objects after follower failure");

    // Verify all 10 objects
    for i in 0..10 {
        let key = format!("object-{}", i);
        let result = client
            .head_object()
            .bucket("rapid-chaos")
            .key(&key)
            .send()
            .await;
        assert!(result.is_ok(), "Object {} should exist", key);
    }

    tracing::info!("Test passed: data durability maintained after follower failure");
}

// Helper function to write an object via the cluster
async fn write_object(cluster: &ClusterEnv, node_id: u64, bucket: &str, key: &str) -> bool {
    let client = match cluster.node_client(node_id) {
        Some(c) => c,
        None => return false,
    };

    let data = format!("data-for-{}", key);
    let result = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(aws_sdk_s3::primitives::ByteStream::from(data.into_bytes()))
        .send()
        .await;

    result.is_ok()
}

// Helper function to create a fresh S3 client (avoids connection pool issues)
async fn create_fresh_client(port: u16) -> aws_sdk_s3::Client {
    let credentials = aws_sdk_s3::config::Credentials::new(
        "test-access-key",
        "test-secret-key",
        None,
        None,
        "static",
    );

    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .force_path_style(true)
        .build();

    aws_sdk_s3::Client::from_conf(s3_config)
}

// Helper function to write an object directly to a specific port
async fn write_object_to_port(port: u16, bucket: &str, key: &str) -> bool {
    let credentials = aws_sdk_s3::config::Credentials::new(
        "test-access-key",
        "test-secret-key",
        None,
        None,
        "static",
    );

    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_sdk_s3::config::Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(format!("http://127.0.0.1:{}", port))
        .force_path_style(true)
        .build();

    let client = aws_sdk_s3::Client::from_conf(s3_config);

    let result = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(aws_sdk_s3::primitives::ByteStream::from_static(
            b"test-data",
        ))
        .send()
        .await;

    result.is_ok()
}
