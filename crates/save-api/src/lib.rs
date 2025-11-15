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

    Router::new()
        .merge(routes::health::routes().with_state(state.clone()))
        .merge(api_routes)
        .layer(axum::middleware::from_fn_with_state(
            state,
            middleware::track_requests,
        ))
        .layer(axum::middleware::from_fn(middleware::track_metrics))
        .layer(axum::middleware::from_fn(middleware::request_id))
}
