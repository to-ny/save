use axum::{routing::put, Router};

use crate::handlers::bucket::create_bucket;
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/{bucket}", put(create_bucket))
}