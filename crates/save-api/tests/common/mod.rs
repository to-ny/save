#![allow(dead_code)]

use axum::http::Request;
use chrono::Utc;
use hmac::{Hmac, Mac};
use save_api::AppState;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_storage::ObjectStorage;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tempfile::TempDir;

type HmacSha256 = Hmac<Sha256>;

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

pub fn sign_request(
    method: &str,
    uri: &str,
    body: &[u8],
    access_key: &str,
    secret_key: &str,
) -> (String, String, String) {
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    let region = "us-east-1";
    let service = "s3";

    let mut hasher = Sha256::new();
    hasher.update(body);
    let payload_hash = hex::encode(hasher.finalize());

    let uri_parts: Vec<&str> = uri.splitn(2, '?').collect();
    let path = uri_parts[0];
    let query = uri_parts.get(1).copied().unwrap_or("");

    let host = "localhost:9000";
    let mut canonical_headers_map: BTreeMap<String, String> = BTreeMap::new();
    canonical_headers_map.insert("host".to_string(), host.to_string());
    canonical_headers_map.insert("x-amz-content-sha256".to_string(), payload_hash.clone());
    canonical_headers_map.insert("x-amz-date".to_string(), amz_date.clone());

    let canonical_headers = canonical_headers_map
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    let signed_headers = "host;x-amz-content-sha256;x-amz-date";

    let canonical_query = if query.is_empty() {
        String::new()
    } else {
        let mut params: Vec<(String, String)> = query
            .split('&')
            .filter_map(|param| {
                let mut parts = param.splitn(2, '=');
                let key = parts.next()?;
                let value = parts.next().unwrap_or("");
                Some((key.to_string(), value.to_string()))
            })
            .collect();
        params.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        params
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    format!("{}=", k)
                } else {
                    format!("{}={}", k, v)
                }
            })
            .collect::<Vec<_>>()
            .join("&")
    };

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n\n{}\n{}",
        method, path, canonical_query, canonical_headers, signed_headers, payload_hash
    );

    let scope = format!("{}/{}/{}/aws4_request", date, region, service);
    let mut hasher = Sha256::new();
    hasher.update(canonical_request.as_bytes());
    let canonical_request_hash = hex::encode(hasher.finalize());
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, scope, canonical_request_hash
    );

    let k_secret = format!("AWS4{}", secret_key);
    let k_date = hmac_sha256(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature_bytes = hmac_sha256(&k_signing, string_to_sign.as_bytes());
    let signature = hex::encode(signature_bytes);

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        access_key, scope, signed_headers, signature
    );

    (authorization, amz_date, payload_hash)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn create_signed_headers(method: &str, uri: &str, body: &[u8]) -> Vec<(String, String)> {
    let (authorization, amz_date, payload_hash) =
        sign_request(method, uri, body, "saveadmin", "savepass");

    vec![
        ("Authorization".to_string(), authorization),
        ("x-amz-date".to_string(), amz_date),
        ("x-amz-content-sha256".to_string(), payload_hash),
        ("host".to_string(), "localhost:9000".to_string()),
    ]
}

pub fn auth_header() -> (&'static str, &'static str) {
    (
        "Authorization",
        "AWS4-HMAC-SHA256 Credential=saveadmin/20231201/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=fake",
    )
}

pub fn request_with_auth<B>(method: &str, uri: &str, body: B) -> Request<B> {
    let (authorization, amz_date, payload_hash) =
        sign_request(method, uri, &[], "saveadmin", "savepass");

    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", authorization)
        .header("x-amz-date", amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("host", "localhost:9000")
        .body(body)
        .unwrap()
}

pub fn request_with_auth_and_body(
    method: &str,
    uri: &str,
    body_bytes: Vec<u8>,
) -> Request<axum::body::Body> {
    let (authorization, amz_date, payload_hash) =
        sign_request(method, uri, &body_bytes, "saveadmin", "savepass");

    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", authorization)
        .header("x-amz-date", amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("host", "localhost:9000")
        .body(axum::body::Body::from(body_bytes))
        .unwrap()
}

pub fn parse_xml(xml: &str) -> roxmltree::Document<'_> {
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
