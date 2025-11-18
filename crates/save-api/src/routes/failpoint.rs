use crate::handlers::failpoint::{configure_failpoint, remove_failpoint};
use crate::state::AppState;
use axum::{Router, routing::post};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/_failpoint/configure", post(configure_failpoint))
        .route("/_failpoint/remove", post(remove_failpoint))
}
