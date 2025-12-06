//! Cluster integration tests for Raft consensus.
//!
//! These tests verify cluster formation, leader election, and recovery behaviors.
//! Run with: cargo test -p crash-recovery-tests --features cluster_tests cluster

mod common;

use common::cluster::ClusterEnv;
use std::time::Duration;

/// Test: 3-node cluster formation and leader election.
///
/// Verifies that:
/// 1. All three nodes start successfully
/// 2. A leader is elected within the expected timeout
/// 3. All nodes agree on the leader
#[tokio::test]
#[ignore = "requires cluster_tests feature"]
async fn test_3_node_cluster_formation() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    assert_eq!(cluster.node_count(), 3);

    // Verify all nodes are running
    for node_id in cluster.node_ids() {
        assert!(
            cluster.is_node_running(node_id),
            "Node {} should be running",
            node_id
        );
    }

    // Verify a leader was elected
    let leader_id = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Leader elected: node {}", leader_id);

    // Verify all nodes see the same leader
    for node_id in cluster.node_ids() {
        let status = cluster
            .get_node_status(node_id)
            .await
            .expect("Failed to get node status");

        assert_eq!(
            status.current_leader,
            Some(leader_id),
            "Node {} should see leader {}",
            node_id,
            leader_id
        );
    }

    // Verify term is consistent across all nodes
    let mut terms = Vec::new();
    for node_id in cluster.node_ids() {
        let status = cluster.get_node_status(node_id).await.unwrap();
        terms.push(status.current_term);
    }

    let max_term = *terms.iter().max().unwrap();
    for (i, term) in terms.iter().enumerate() {
        assert!(
            *term >= max_term - 1,
            "Node {} term {} too far behind max {}",
            i + 1,
            term,
            max_term
        );
    }

    tracing::info!("Test passed: 3-node cluster formation successful");
}

/// Test: Leader election after leader crash.
///
/// Verifies that:
/// 1. After killing the leader, a new leader is elected
/// 2. The new leader is one of the remaining nodes
/// 3. The cluster continues to function
#[tokio::test]
#[ignore = "requires cluster_tests feature"]
async fn test_leader_election_after_crash() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let mut cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    // Get the initial leader
    let initial_leader = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Initial leader: node {}", initial_leader);

    // Get the initial term
    let initial_status = cluster
        .get_node_status(initial_leader)
        .await
        .expect("Failed to get status");
    let initial_term = initial_status.current_term;

    // Kill the leader
    cluster
        .kill_node(initial_leader)
        .expect("Failed to kill leader");
    tracing::info!("Killed leader node {}", initial_leader);

    // Wait for new leader election (may take a few election timeouts)
    let new_leader = cluster
        .wait_for_leader(Duration::from_secs(30))
        .await
        .expect("Should elect new leader");

    tracing::info!("New leader elected: node {}", new_leader);

    // Verify the new leader is different from the killed leader
    assert_ne!(
        new_leader, initial_leader,
        "New leader should be different from crashed leader"
    );

    // Verify the new leader is one of the remaining nodes
    assert!(
        cluster.is_node_running(new_leader),
        "New leader should be running"
    );

    // Verify term increased (indicates election occurred)
    let new_status = cluster
        .get_node_status(new_leader)
        .await
        .expect("Failed to get new leader status");
    assert!(
        new_status.current_term > initial_term,
        "Term should have increased after leader election"
    );

    // Verify the other follower also sees the new leader
    let remaining_nodes: Vec<u64> = cluster
        .node_ids()
        .into_iter()
        .filter(|id| *id != initial_leader && *id != new_leader)
        .collect();

    for follower_id in remaining_nodes {
        if cluster.is_node_running(follower_id) {
            let status = cluster
                .get_node_status(follower_id)
                .await
                .expect("Failed to get follower status");
            assert_eq!(
                status.current_leader,
                Some(new_leader),
                "Follower {} should see new leader {}",
                follower_id,
                new_leader
            );
        }
    }

    tracing::info!("Test passed: leader election after crash successful");
}

/// Test: Node recovery and catch-up.
///
/// Verifies that:
/// 1. A crashed node can restart and rejoin the cluster
/// 2. The restarted node catches up with the cluster state
/// 3. The restarted node sees the current leader
#[tokio::test]
#[ignore = "requires cluster_tests feature"]
async fn test_node_recovery_and_catchup() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let mut cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    // Get the initial leader
    let leader = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Initial leader: node {}", leader);

    // Pick a follower to crash and recover
    let follower_id = cluster
        .node_ids()
        .into_iter()
        .find(|id| *id != leader)
        .expect("Should have a follower");

    // Get the follower's state before crash
    let pre_crash_status = cluster
        .get_node_status(follower_id)
        .await
        .expect("Failed to get follower status");
    tracing::info!(
        "Pre-crash follower {} state: term={}, leader={:?}",
        follower_id,
        pre_crash_status.current_term,
        pre_crash_status.current_leader
    );

    // Kill the follower
    cluster
        .kill_node(follower_id)
        .expect("Failed to kill follower");
    tracing::info!("Killed follower node {}", follower_id);

    // Wait a bit for the cluster to notice the node is down
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Verify the cluster still has a leader
    assert!(
        cluster.get_leader().await.is_some(),
        "Cluster should still have a leader with 2/3 nodes"
    );

    // Restart the crashed node
    cluster
        .restart_node(follower_id)
        .await
        .expect("Failed to restart follower");
    tracing::info!("Restarted follower node {}", follower_id);

    // Give the node time to catch up
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify the restarted node sees the current leader
    let post_restart_status = cluster
        .get_node_status(follower_id)
        .await
        .expect("Failed to get restarted follower status");

    let current_leader = cluster.get_leader().await.expect("Should have a leader");

    assert_eq!(
        post_restart_status.current_leader,
        Some(current_leader),
        "Restarted node should see the current leader"
    );

    // Verify the term is caught up
    let leader_status = cluster
        .get_node_status(current_leader)
        .await
        .expect("Failed to get leader status");

    assert!(
        post_restart_status.current_term >= leader_status.current_term - 1,
        "Restarted node term should be caught up"
    );

    tracing::info!(
        "Post-restart follower {} state: term={}, leader={:?}",
        follower_id,
        post_restart_status.current_term,
        post_restart_status.current_leader
    );

    tracing::info!("Test passed: node recovery and catch-up successful");
}

/// Test: Cluster survives minority failure.
///
/// Verifies that:
/// 1. With 1 of 3 nodes down, the cluster can still function
/// 2. The remaining 2 nodes maintain quorum
#[tokio::test]
#[ignore = "requires cluster_tests feature"]
async fn test_cluster_survives_minority_failure() {
    tracing_subscriber::fmt()
        .with_env_filter("info,save_metadata::raft=debug")
        .try_init()
        .ok();

    // Create a 3-node cluster
    let mut cluster = ClusterEnv::new_3_node()
        .await
        .expect("Failed to create cluster");

    // Get the initial leader
    let leader = cluster.get_leader().await.expect("Should have a leader");
    tracing::info!("Initial leader: node {}", leader);

    // Pick a follower to kill (not the leader)
    let follower_id = cluster
        .node_ids()
        .into_iter()
        .find(|id| *id != leader)
        .expect("Should have a follower");

    // Kill one follower
    cluster
        .kill_node(follower_id)
        .expect("Failed to kill follower");
    tracing::info!("Killed follower node {}", follower_id);

    // Wait a bit
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Verify the cluster still has a leader (quorum of 2/3)
    let current_leader = cluster.get_leader().await;
    assert!(
        current_leader.is_some(),
        "Cluster should still have a leader with 2/3 nodes"
    );
    tracing::info!(
        "Cluster still operational with leader: {:?}",
        current_leader
    );

    // Verify we can still get status from the remaining nodes
    for node_id in cluster.node_ids() {
        if cluster.is_node_running(node_id) {
            let status = cluster.get_node_status(node_id).await;
            assert!(
                status.is_ok(),
                "Running node {} should respond to status request",
                node_id
            );
        }
    }

    tracing::info!("Test passed: cluster survives minority failure");
}
