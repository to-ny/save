use axum::Router;

pub mod auth;
pub mod gc;
pub mod handlers;
pub mod metrics;
pub mod middleware;
pub mod routes;
pub mod state;

#[cfg(test)]
pub mod test_helpers;

pub use gc::{GcConfig, run_gc_worker};
pub use middleware::RequestTracker;
pub use state::AppState;

pub async fn collect_metrics(state: &AppState) {
    use std::path::Path;

    let data_path = Path::new(&state.config.storage.data_path);
    let metadata_path = Path::new(&state.config.storage.metadata_path);

    metrics::collect_disk_usage(data_path, "data");
    metrics::collect_disk_usage(metadata_path, "metadata");

    let db_stats = state.metadata.get_stats();
    metrics::collect_database_stats(&db_stats);

    let temp_dir = data_path.join("temp");
    metrics::collect_temp_file_stats(&temp_dir);

    // Collect Raft metrics
    collect_raft_metrics_from_node(state);

    // TODO: Cluster health metrics (node status, quorum, partition) are collected
    // when ClusterManager is wired up. Currently Raft metrics are collected from
    // the RaftNode. When ClusterManager is added to AppState, call:
    // collect_cluster_health_from_manager(state);
}

fn collect_raft_metrics_from_node(state: &AppState) {
    let status = state.raft_node.get_status();

    let data = metrics::RaftMetrics {
        term: status.current_term,
        state: status.state,
        last_applied_index: status.last_applied_index,
        last_log_index: status.last_log_index,
        voters_count: status.voters.len(),
        learners_count: status.learners.len(),
    };

    metrics::collect_raft_metrics(&data);
}

pub fn app(state: AppState) -> Router {
    let api_routes = routes::bucket::routes()
        .merge(routes::multipart::routes())
        .merge(routes::objects::routes())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::validate_sigv4,
        ))
        .with_state(state.clone());

    // Build router with specific routes first, then use fallback for catch-all patterns
    #[cfg_attr(not(feature = "failpoints"), allow(unused_mut))]
    let mut router = Router::new()
        .merge(routes::health::routes().with_state(state.clone()))
        .merge(routes::cluster::routes().with_state(state.clone()));

    #[cfg(feature = "failpoints")]
    {
        router = router.merge(routes::failpoint::routes().with_state(state.clone()));
    }

    // Use fallback_service for API routes so specific routes take precedence
    router = router.fallback_service(api_routes);

    router
        .layer(axum::middleware::from_fn_with_state(
            state,
            middleware::track_requests,
        ))
        .layer(axum::middleware::from_fn(middleware::track_metrics))
        .layer(axum::middleware::from_fn(middleware::request_id))
}
