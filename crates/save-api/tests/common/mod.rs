#![allow(dead_code)]

use axum::http::Request;
use save_api::AppState;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use tempfile::TempDir;

pub async fn setup_empty() -> (AppState, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let data_path = temp_dir.path().join("data");
    let metadata_path = temp_dir.path().join("metadata");

    let mut config = SaveConfig::default();
    config.storage.data_path = data_path.to_str().unwrap().to_string();
    config.storage.metadata_path = metadata_path.to_str().unwrap().to_string();

    let storage = ObjectStorage::new(&config.storage.data_path).await.unwrap();
    let metadata = MetadataStore::new(&config.storage.metadata_path).unwrap();

    let state = AppState::new(storage, metadata, config);
    (state, temp_dir)
}

pub async fn setup() -> (AppState, TempDir) {
    let (state, temp_dir) = setup_empty().await;
    state.metadata.create_bucket("test-bucket").await.unwrap();
    (state, temp_dir)
}

pub async fn test_setup() -> (AppState, TempDir) {
    setup().await
}

pub fn auth_header() -> (&'static str, &'static str) {
    (
        "Authorization",
        "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake",
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

pub fn parse_xml(xml: &str) -> roxmltree::Document {
    roxmltree::Document::parse(xml).expect("Failed to parse XML response")
}

pub fn get_element_text<'a>(node: roxmltree::Node<'a, 'a>, tag: &str) -> Option<&'a str> {
    node.descendants()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
}

pub fn get_all_elements<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    tag: &str,
) -> Vec<roxmltree::Node<'a, 'input>> {
    node.descendants().filter(|n| n.has_tag_name(tag)).collect()
}
