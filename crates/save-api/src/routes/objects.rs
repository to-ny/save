use axum::{
    routing::get,
    Router,
};

use crate::handlers::objects::{get_object, put_object};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/{bucket}/{*key}",
        get(get_object).put(put_object),
    )
}
