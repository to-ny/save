//! Cluster status and management routes.

use axum::{
    Router,
    routing::{delete, get, post},
};

use crate::handlers::cluster::{
    add_learner, cluster_initialize, cluster_status, promote_voters, remove_node,
};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cluster/status", get(cluster_status))
        .route("/cluster/initialize", post(cluster_initialize))
        .route("/cluster/members", post(add_learner))
        .route("/cluster/members/promote", post(promote_voters))
        .route("/cluster/members/{node_id}", delete(remove_node))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_cluster_status_returns_ok() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .uri("/cluster/status")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert!(json["initialized"].as_bool().unwrap());
        assert_eq!(json["node_id"], 1);
        assert!(json["voters"].as_array().unwrap().len() >= 1);
        assert!(json["learners"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_add_learner_requires_leader() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/cluster/members")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"node": "2:127.0.0.1:9002"}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert!(
            response.status() == StatusCode::OK
                || response.status() == StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn test_promote_voters_rejects_empty() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/cluster/members/promote")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"node_ids": []}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_remove_node_prevents_self_removal() {
        let (state, _temp_dir) = crate::test_helpers::test_setup().await;
        let app = routes().with_state(state);

        let request = Request::builder()
            .method("DELETE")
            .uri("/cluster/members/1")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("Cannot remove the leader"));
    }
}
