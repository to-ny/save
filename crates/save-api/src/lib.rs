use axum::Router;

pub mod routes;

pub fn app() -> Router {
    Router::new()
        .merge(routes::health::routes())
}
