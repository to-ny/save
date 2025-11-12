use axum::body::Body;
use axum::http::{Request, StatusCode};
use save_api::AppState;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;
use tower::ServiceExt;

async fn setup() -> (AppState, TempDir, TempDir) {
    let data_dir = TempDir::new().unwrap();
    let metadata_dir = TempDir::new().unwrap();

    let storage = ObjectStorage::new(data_dir.path().to_str().unwrap())
        .await
        .unwrap();
    let metadata = MetadataStore::new(metadata_dir.path().to_str().unwrap()).unwrap();
    let config = SaveConfig::default();

    let state = AppState::new(storage, metadata, config);

    (state, data_dir, metadata_dir)
}

#[tokio::test]
async fn test_request_tracker_increments_and_decrements() {
    let (state, _data_dir, _metadata_dir) = setup().await;

    assert_eq!(state.request_tracker.in_flight_count(), 0);

    let app = save_api::app(state.clone());

    let request = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    assert_eq!(state.request_tracker.in_flight_count(), 0);
}

#[tokio::test]
async fn test_concurrent_requests_tracked() {
    let (state, _data_dir, _metadata_dir) = setup().await;

    let app = save_api::app(state.clone());

    let requests = (0..10).map(|_| {
        let app_clone = app.clone();
        tokio::spawn(async move {
            let request = Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap();
            app_clone.oneshot(request).await
        })
    });

    let results = futures::future::join_all(requests).await;

    for result in results {
        let response = result.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    assert_eq!(state.request_tracker.in_flight_count(), 0);
}

#[tokio::test]
async fn test_multiple_slow_requests_tracked() {
    let (state, _data_dir, _metadata_dir) = setup().await;

    let initial_count = state.request_tracker.in_flight_count();
    assert_eq!(initial_count, 0);

    let app = save_api::app(state.clone());

    let handles: Vec<_> = (0..5)
        .map(|_| {
            let app_clone = app.clone();
            tokio::spawn(async move {
                let request = Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap();
                sleep(Duration::from_millis(50)).await;
                app_clone.oneshot(request).await
            })
        })
        .collect();

    sleep(Duration::from_millis(10)).await;

    for handle in handles {
        let response = handle.await.unwrap().unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    assert_eq!(state.request_tracker.in_flight_count(), 0);
}
