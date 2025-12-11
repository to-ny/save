//! Cluster, Raft, and replication metrics.

use prometheus::{
    GaugeVec, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, register_gauge_vec,
    register_histogram_vec, register_int_counter, register_int_counter_vec, register_int_gauge,
    register_int_gauge_vec,
};
use save_metadata::raft::RaftState;
use save_storage::cluster::{NodeHealth, PartitionStatus};
use std::sync::OnceLock;

// Raft consensus metrics
static RAFT_TERM: OnceLock<IntGauge> = OnceLock::new();
static RAFT_STATE: OnceLock<IntGaugeVec> = OnceLock::new();
static RAFT_LEADER_ELECTIONS_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static RAFT_LOG_INDEX: OnceLock<IntGaugeVec> = OnceLock::new();
static RAFT_SNAPSHOT_DURATION_SECONDS: OnceLock<HistogramVec> = OnceLock::new();
static RAFT_PROPOSALS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static RAFT_MEMBERS: OnceLock<IntGaugeVec> = OnceLock::new();

// Replication metrics
static REPLICATION_WRITES_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static REPLICATION_READS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static REPLICATION_BYTES_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static REPLICATION_QUORUM_RESULTS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static REPLICATION_LATENCY_SECONDS: OnceLock<HistogramVec> = OnceLock::new();

// Cluster health metrics
static CLUSTER_NODE_STATUS: OnceLock<IntGaugeVec> = OnceLock::new();
static CLUSTER_QUORUM_STATUS: OnceLock<IntGauge> = OnceLock::new();
static CLUSTER_PARTITION_STATUS: OnceLock<IntGaugeVec> = OnceLock::new();
static CLUSTER_REPLICATION_LAG_SECONDS: OnceLock<GaugeVec> = OnceLock::new();

// Replica metrics
static REPLICA_COUNT: OnceLock<IntGaugeVec> = OnceLock::new();
static UNDER_REPLICATED_OBJECTS_TOTAL: OnceLock<IntGauge> = OnceLock::new();

// Request forwarding metrics
static REQUESTS_FORWARDED_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static FORWARDING_LATENCY_SECONDS: OnceLock<HistogramVec> = OnceLock::new();

// =============================================================================
// Raft State Label (uses From trait)
// =============================================================================

const RAFT_STATE_LABELS: &[&str] = &["leader", "follower", "candidate", "learner", "shutdown"];

fn raft_state_to_label(state: RaftState) -> &'static str {
    match state {
        RaftState::Leader => "leader",
        RaftState::Follower => "follower",
        RaftState::Candidate => "candidate",
        RaftState::Learner => "learner",
        RaftState::Shutdown => "shutdown",
    }
}

// =============================================================================
// Node Health Label (uses From trait)
// =============================================================================

const NODE_HEALTH_LABELS: &[&str] = &["healthy", "degraded", "unreachable", "unknown"];

fn node_health_to_label(health: NodeHealth) -> &'static str {
    match health {
        NodeHealth::Healthy => "healthy",
        NodeHealth::Degraded => "degraded",
        NodeHealth::Unreachable => "unreachable",
        NodeHealth::Unknown => "unknown",
    }
}

// =============================================================================
// Partition Status Label (uses From trait)
// =============================================================================

const PARTITION_STATUS_LABELS: &[&str] = &["connected", "minority", "possible_minority", "unknown"];

fn partition_status_to_label(status: PartitionStatus) -> &'static str {
    match status {
        PartitionStatus::Connected => "connected",
        PartitionStatus::Minority => "minority",
        PartitionStatus::PossibleMinority => "possible_minority",
        PartitionStatus::Unknown => "unknown",
    }
}

// =============================================================================
// Raft Consensus Metrics
// =============================================================================

/// Current Raft term number.
pub fn raft_term() -> &'static IntGauge {
    RAFT_TERM.get_or_init(|| {
        register_int_gauge!("save_raft_term", "Current Raft term number")
            .expect("Failed to register save_raft_term metric")
    })
}

/// Raft node state (1 = active for the labeled state, 0 otherwise).
pub fn raft_state() -> &'static IntGaugeVec {
    RAFT_STATE.get_or_init(|| {
        register_int_gauge_vec!(
            "save_raft_state",
            "Raft node state (1 = current state)",
            &["state"]
        )
        .expect("Failed to register save_raft_state metric")
    })
}

/// Total leader elections observed by this node.
pub fn raft_leader_elections_total() -> &'static IntCounter {
    RAFT_LEADER_ELECTIONS_TOTAL.get_or_init(|| {
        register_int_counter!(
            "save_raft_leader_elections_total",
            "Total number of leader elections observed"
        )
        .expect("Failed to register save_raft_leader_elections_total metric")
    })
}

/// Raft log indices by type.
pub fn raft_log_index() -> &'static IntGaugeVec {
    RAFT_LOG_INDEX.get_or_init(|| {
        register_int_gauge_vec!("save_raft_log_index", "Raft log index by type", &["type"])
            .expect("Failed to register save_raft_log_index metric")
    })
}

/// Raft snapshot operation duration.
pub fn raft_snapshot_duration_seconds() -> &'static HistogramVec {
    RAFT_SNAPSHOT_DURATION_SECONDS.get_or_init(|| {
        register_histogram_vec!(
            "save_raft_snapshot_duration_seconds",
            "Raft snapshot operation duration in seconds",
            &["operation"],
            vec![0.1, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0]
        )
        .expect("Failed to register save_raft_snapshot_duration_seconds metric")
    })
}

/// Total Raft proposals by result.
pub fn raft_proposals_total() -> &'static IntCounterVec {
    RAFT_PROPOSALS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_raft_proposals_total",
            "Total Raft proposals by result",
            &["result"]
        )
        .expect("Failed to register save_raft_proposals_total metric")
    })
}

/// Raft cluster membership counts.
pub fn raft_members() -> &'static IntGaugeVec {
    RAFT_MEMBERS.get_or_init(|| {
        register_int_gauge_vec!(
            "save_raft_members",
            "Number of Raft cluster members by type",
            &["type"]
        )
        .expect("Failed to register save_raft_members metric")
    })
}

// =============================================================================
// Replication Metrics
// =============================================================================

/// Total replication writes by node and result.
pub fn replication_writes_total() -> &'static IntCounterVec {
    REPLICATION_WRITES_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_replication_writes_total",
            "Total replication writes by node and result",
            &["node_id", "result"]
        )
        .expect("Failed to register save_replication_writes_total metric")
    })
}

/// Total replication reads by node and result.
pub fn replication_reads_total() -> &'static IntCounterVec {
    REPLICATION_READS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_replication_reads_total",
            "Total replication reads by node and result",
            &["node_id", "result"]
        )
        .expect("Failed to register save_replication_reads_total metric")
    })
}

/// Total bytes replicated by direction.
pub fn replication_bytes_total() -> &'static IntCounterVec {
    REPLICATION_BYTES_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_replication_bytes_total",
            "Total bytes replicated by direction",
            &["direction"]
        )
        .expect("Failed to register save_replication_bytes_total metric")
    })
}

/// Quorum operation results.
pub fn replication_quorum_results_total() -> &'static IntCounterVec {
    REPLICATION_QUORUM_RESULTS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_replication_quorum_results_total",
            "Total quorum operation results",
            &["operation", "result"]
        )
        .expect("Failed to register save_replication_quorum_results_total metric")
    })
}

/// Replication operation latency.
pub fn replication_latency_seconds() -> &'static HistogramVec {
    REPLICATION_LATENCY_SECONDS.get_or_init(|| {
        register_histogram_vec!(
            "save_replication_latency_seconds",
            "Replication operation latency in seconds",
            &["operation"],
            vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0
            ]
        )
        .expect("Failed to register save_replication_latency_seconds metric")
    })
}

// =============================================================================
// Cluster Health Metrics
// =============================================================================

/// Node health status (1 = matches label, 0 otherwise).
pub fn cluster_node_status() -> &'static IntGaugeVec {
    CLUSTER_NODE_STATUS.get_or_init(|| {
        register_int_gauge_vec!(
            "save_cluster_node_status",
            "Cluster node health status",
            &["node_id", "status"]
        )
        .expect("Failed to register save_cluster_node_status metric")
    })
}

/// Whether cluster has quorum (1 = yes, 0 = no).
pub fn cluster_quorum_status() -> &'static IntGauge {
    CLUSTER_QUORUM_STATUS.get_or_init(|| {
        register_int_gauge!(
            "save_cluster_quorum_status",
            "Whether cluster has quorum (1 = yes, 0 = no)"
        )
        .expect("Failed to register save_cluster_quorum_status metric")
    })
}

/// Partition status (1 = matches label, 0 otherwise).
pub fn cluster_partition_status() -> &'static IntGaugeVec {
    CLUSTER_PARTITION_STATUS.get_or_init(|| {
        register_int_gauge_vec!(
            "save_cluster_partition_status",
            "Cluster partition status",
            &["status"]
        )
        .expect("Failed to register save_cluster_partition_status metric")
    })
}

/// Replication lag in seconds by node.
pub fn cluster_replication_lag_seconds() -> &'static GaugeVec {
    CLUSTER_REPLICATION_LAG_SECONDS.get_or_init(|| {
        register_gauge_vec!(
            "save_cluster_replication_lag_seconds",
            "Replication lag in seconds by node",
            &["node_id"]
        )
        .expect("Failed to register save_cluster_replication_lag_seconds metric")
    })
}

// =============================================================================
// Replica Metrics
// =============================================================================

/// Number of object replicas by bucket.
pub fn replica_count() -> &'static IntGaugeVec {
    REPLICA_COUNT.get_or_init(|| {
        register_int_gauge_vec!(
            "save_replica_count",
            "Number of objects by replica count per bucket",
            &["bucket", "replica_count"]
        )
        .expect("Failed to register save_replica_count metric")
    })
}

/// Total number of under-replicated objects across all buckets.
pub fn under_replicated_objects_total() -> &'static IntGauge {
    UNDER_REPLICATED_OBJECTS_TOTAL.get_or_init(|| {
        register_int_gauge!(
            "save_under_replicated_objects_total",
            "Total number of under-replicated objects"
        )
        .expect("Failed to register save_under_replicated_objects_total metric")
    })
}

// =============================================================================
// Collection Functions
// =============================================================================

/// Raft metrics data for collection.
pub struct RaftMetrics {
    pub term: u64,
    pub state: RaftState,
    pub last_applied_index: Option<u64>,
    pub last_log_index: Option<u64>,
    pub voters_count: usize,
    pub learners_count: usize,
}

/// Collect Raft consensus metrics.
pub fn collect_raft_metrics(data: &RaftMetrics) {
    raft_term().set(data.term as i64);

    // Set current state to 1, all others to 0
    let current_label = raft_state_to_label(data.state);
    for label in RAFT_STATE_LABELS {
        let value = if *label == current_label { 1 } else { 0 };
        raft_state().with_label_values(&[label]).set(value);
    }

    if let Some(idx) = data.last_applied_index {
        raft_log_index()
            .with_label_values(&["last_applied"])
            .set(idx as i64);
    }
    if let Some(idx) = data.last_log_index {
        raft_log_index()
            .with_label_values(&["last_log"])
            .set(idx as i64);
    }

    raft_members()
        .with_label_values(&["voters"])
        .set(data.voters_count as i64);
    raft_members()
        .with_label_values(&["learners"])
        .set(data.learners_count as i64);
}

/// Record a Raft proposal result.
pub fn record_raft_proposal(result: &str) {
    raft_proposals_total().with_label_values(&[result]).inc();
}

/// Record a Raft snapshot operation duration.
pub fn record_raft_snapshot_duration(operation: &str, duration_secs: f64) {
    raft_snapshot_duration_seconds()
        .with_label_values(&[operation])
        .observe(duration_secs);
}

/// Cluster node metrics data.
pub struct ClusterNodeMetrics {
    pub node_id: u64,
    pub health: NodeHealth,
}

/// Collect cluster health metrics.
pub fn collect_cluster_health_metrics(
    nodes: &[ClusterNodeMetrics],
    has_quorum: bool,
    partition_status: PartitionStatus,
) {
    cluster_quorum_status().set(if has_quorum { 1 } else { 0 });

    // Set partition status (1 for current, 0 for others)
    let current_partition = partition_status_to_label(partition_status);
    for label in PARTITION_STATUS_LABELS {
        let value = if *label == current_partition { 1 } else { 0 };
        cluster_partition_status()
            .with_label_values(&[label])
            .set(value);
    }

    // Set node health status
    for node in nodes {
        let node_id_str = node.node_id.to_string();
        let current_health = node_health_to_label(node.health);
        for label in NODE_HEALTH_LABELS {
            let value = if *label == current_health { 1 } else { 0 };
            cluster_node_status()
                .with_label_values(&[node_id_str.as_str(), label])
                .set(value);
        }
    }
}

/// Record replication lag for a node.
pub fn record_replication_lag(node_id: u64, lag_secs: f64) {
    cluster_replication_lag_seconds()
        .with_label_values(&[&node_id.to_string()])
        .set(lag_secs);
}

/// Record a replication write result.
pub fn record_replication_write(node_id: u64, success: bool) {
    let node_id_str = node_id.to_string();
    let result = if success { "success" } else { "failed" };
    replication_writes_total()
        .with_label_values(&[node_id_str.as_str(), result])
        .inc();
}

/// Record a replication read result.
pub fn record_replication_read(node_id: u64, success: bool) {
    let node_id_str = node_id.to_string();
    let result = if success { "success" } else { "failed" };
    replication_reads_total()
        .with_label_values(&[node_id_str.as_str(), result])
        .inc();
}

/// Record bytes replicated.
pub fn record_replication_bytes(direction: &str, bytes: u64) {
    replication_bytes_total()
        .with_label_values(&[direction])
        .inc_by(bytes);
}

/// Record a quorum operation result.
pub fn record_quorum_result(operation: &str, achieved: bool) {
    let result = if achieved { "achieved" } else { "failed" };
    replication_quorum_results_total()
        .with_label_values(&[operation, result])
        .inc();
}

/// Record replication operation latency.
pub fn record_replication_latency(operation: &str, duration_secs: f64) {
    replication_latency_seconds()
        .with_label_values(&[operation])
        .observe(duration_secs);
}

/// Replica count data for a bucket.
pub struct BucketReplicaMetrics {
    pub bucket: String,
    pub objects_by_replica_count: Vec<(usize, i64)>,
}

/// Collect replica metrics.
pub fn collect_replica_metrics(buckets: &[BucketReplicaMetrics], under_replicated_total: i64) {
    for bucket_metrics in buckets {
        for (replica_count_val, object_count) in &bucket_metrics.objects_by_replica_count {
            replica_count()
                .with_label_values(&[&bucket_metrics.bucket, &replica_count_val.to_string()])
                .set(*object_count);
        }
    }
    under_replicated_objects_total().set(under_replicated_total);
}

// =============================================================================
// Request Forwarding Metrics
// =============================================================================

/// Total requests forwarded to leader by result.
pub fn requests_forwarded_total() -> &'static IntCounterVec {
    REQUESTS_FORWARDED_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_requests_forwarded_total",
            "Total requests forwarded to leader by result",
            &["result"]
        )
        .expect("Failed to register save_requests_forwarded_total metric")
    })
}

/// Request forwarding latency.
pub fn forwarding_latency_seconds() -> &'static HistogramVec {
    FORWARDING_LATENCY_SECONDS.get_or_init(|| {
        register_histogram_vec!(
            "save_forwarding_latency_seconds",
            "Request forwarding latency in seconds",
            &["method"],
            vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0
            ]
        )
        .expect("Failed to register save_forwarding_latency_seconds metric")
    })
}

/// Record a forwarded request result.
pub fn record_forwarded_request(success: bool) {
    let result = if success { "success" } else { "failed" };
    requests_forwarded_total()
        .with_label_values(&[result])
        .inc();
}

/// Record forwarding latency.
pub fn record_forwarding_latency(method: &str, duration_secs: f64) {
    forwarding_latency_seconds()
        .with_label_values(&[method])
        .observe(duration_secs);
}

pub(crate) fn init() {
    // Raft metrics
    let _ = raft_term();
    let _ = raft_state();
    let _ = raft_leader_elections_total();
    let _ = raft_log_index();
    let _ = raft_snapshot_duration_seconds();
    let _ = raft_proposals_total();
    let _ = raft_members();

    // Replication metrics
    let _ = replication_writes_total();
    let _ = replication_reads_total();
    let _ = replication_bytes_total();
    let _ = replication_quorum_results_total();
    let _ = replication_latency_seconds();

    // Cluster health metrics
    let _ = cluster_node_status();
    let _ = cluster_quorum_status();
    let _ = cluster_partition_status();
    let _ = cluster_replication_lag_seconds();

    // Replica metrics
    let _ = replica_count();
    let _ = under_replicated_objects_total();

    // Forwarding metrics
    let _ = requests_forwarded_total();
    let _ = forwarding_latency_seconds();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raft_state_to_label() {
        assert_eq!(raft_state_to_label(RaftState::Leader), "leader");
        assert_eq!(raft_state_to_label(RaftState::Follower), "follower");
        assert_eq!(raft_state_to_label(RaftState::Candidate), "candidate");
        assert_eq!(raft_state_to_label(RaftState::Learner), "learner");
        assert_eq!(raft_state_to_label(RaftState::Shutdown), "shutdown");
    }

    #[test]
    fn test_node_health_to_label() {
        assert_eq!(node_health_to_label(NodeHealth::Healthy), "healthy");
        assert_eq!(node_health_to_label(NodeHealth::Degraded), "degraded");
        assert_eq!(node_health_to_label(NodeHealth::Unreachable), "unreachable");
        assert_eq!(node_health_to_label(NodeHealth::Unknown), "unknown");
    }

    #[test]
    fn test_partition_status_to_label() {
        assert_eq!(
            partition_status_to_label(PartitionStatus::Connected),
            "connected"
        );
        assert_eq!(
            partition_status_to_label(PartitionStatus::Minority),
            "minority"
        );
        assert_eq!(
            partition_status_to_label(PartitionStatus::PossibleMinority),
            "possible_minority"
        );
        assert_eq!(
            partition_status_to_label(PartitionStatus::Unknown),
            "unknown"
        );
    }

    #[test]
    fn test_raft_metrics_collection() {
        let data = RaftMetrics {
            term: 42,
            state: RaftState::Leader,
            last_applied_index: Some(100),
            last_log_index: Some(105),
            voters_count: 3,
            learners_count: 1,
        };

        collect_raft_metrics(&data);

        assert_eq!(raft_term().get(), 42);
        assert_eq!(raft_state().with_label_values(&["leader"]).get(), 1);
        assert_eq!(raft_state().with_label_values(&["follower"]).get(), 0);
    }

    #[test]
    fn test_cluster_health_collection() {
        let nodes = vec![
            ClusterNodeMetrics {
                node_id: 1,
                health: NodeHealth::Healthy,
            },
            ClusterNodeMetrics {
                node_id: 2,
                health: NodeHealth::Degraded,
            },
        ];

        collect_cluster_health_metrics(&nodes, true, PartitionStatus::Connected);

        assert_eq!(cluster_quorum_status().get(), 1);
        assert_eq!(
            cluster_partition_status()
                .with_label_values(&["connected"])
                .get(),
            1
        );
    }

    #[test]
    fn test_replication_metrics() {
        record_replication_write(1, true);
        record_replication_write(1, false);
        record_replication_read(2, true);
        record_replication_bytes("sent", 1024);
        record_quorum_result("write", true);
        record_replication_latency("prepare", 0.05);
    }

    #[test]
    fn test_replica_metrics() {
        let buckets = vec![BucketReplicaMetrics {
            bucket: "test-bucket".to_string(),
            objects_by_replica_count: vec![(3, 100), (2, 5)],
        }];

        collect_replica_metrics(&buckets, 7);
        assert_eq!(under_replicated_objects_total().get(), 7);
    }

    #[test]
    fn test_forwarding_metrics() {
        record_forwarded_request(true);
        record_forwarded_request(false);
        record_forwarding_latency("PUT", 0.05);
        record_forwarding_latency("DELETE", 0.02);

        // Just verify the metrics were recorded without panicking
        let _ = requests_forwarded_total();
        let _ = forwarding_latency_seconds();
    }
}
