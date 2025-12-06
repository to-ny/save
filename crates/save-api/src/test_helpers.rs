use crate::AppState;
use axum::http::Request;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_metadata::raft::RaftNode;
use save_storage::LocalBackend;
use std::sync::atomic::{AtomicU16, Ordering};
use tempfile::TempDir;

/// Atomic counter for generating unique ports in tests
static PORT_COUNTER: AtomicU16 = AtomicU16::new(19000);

/// Get the next available port for test Raft servers
fn next_test_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

pub async fn test_setup_empty() -> (AppState, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let data_path = temp_dir.path().join("data");
    let metadata_path = temp_dir.path().join("metadata");

    let mut config = SaveConfig::default();
    config.storage.data_path = data_path.to_str().unwrap().to_string();
    config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

    // Configure cluster for single-node test
    let raft_port = next_test_port();
    config.cluster.node_id = 1;
    config.cluster.raft_bind_addr = format!("127.0.0.1:{}", raft_port);

    let storage = LocalBackend::new(&config.storage.data_path).await.unwrap();
    let metadata = MetadataStore::new(&config.storage.metadata_path).unwrap();

    // Create RaftNode and auto-bootstrap as single-node cluster
    let raft_node = RaftNode::from_cluster_config(metadata.db(), &config.cluster)
        .await
        .unwrap();

    // Auto-bootstrap single-node cluster
    let raft_addr = format!("http://{}", config.cluster.raft_bind_addr);
    raft_node
        .initialize(vec![(config.cluster.node_id, raft_addr)])
        .await
        .unwrap();

    let state = AppState::new(storage, metadata, config, raft_node);
    (state, temp_dir)
}

pub async fn test_setup() -> (AppState, TempDir) {
    let (state, temp_dir) = test_setup_empty().await;
    state.metadata.create_bucket("test-bucket").await.unwrap();
    (state, temp_dir)
}

pub fn auth_header() -> (&'static str, &'static str) {
    (
        "Authorization",
        "AWS4-HMAC-SHA256 Credential=test-access-key/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake",
    )
}

pub fn request_with_auth<B>(method: &str, uri: &str, body: B) -> Request<B> {
    let (key, value) = auth_header();
    Request::builder()
        .method(method)
        .uri(uri)
        .header(key, value)
        .body(body)
        .unwrap()
}
