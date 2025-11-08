use axum::{middleware, Router};

pub mod auth;
pub mod handlers;
pub mod routes;
pub mod state;

#[cfg(test)]
pub mod test_helpers;

pub use state::AppState;

pub fn app(state: AppState) -> Router {
    let api_routes = routes::objects::routes()
        .merge(routes::multipart::routes())
        .layer(middleware::from_fn_with_state(state.clone(), auth::validate_sigv4))
        .with_state(state);

    Router::new()
        .merge(routes::health::routes())
        .merge(api_routes)
}
