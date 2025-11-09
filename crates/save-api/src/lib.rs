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
pub use state::AppState;

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
        .merge(routes::health::routes().with_state(state))
        .merge(api_routes)
        .layer(axum::middleware::from_fn(middleware::track_metrics))
}
