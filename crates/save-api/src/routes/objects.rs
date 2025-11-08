use axum::{routing::put, Router};

use crate::handlers::objects::put_object;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/{bucket}/{*key}", put(put_object))
}
