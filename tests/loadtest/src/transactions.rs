use crate::config::LoadTestConfig;
use crate::objects::ObjectGenerator;
use crate::signing::{S3Signer, sha256_hex};
use goose::prelude::*;
use std::sync::Arc;

pub struct AppState {
    pub config: LoadTestConfig,
    pub generator: ObjectGenerator,
    pub uploaded_keys: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub signer: S3Signer,
}

impl AppState {
    pub fn new(config: LoadTestConfig) -> Self {
        let signer = S3Signer::new(
            config.target.access_key.clone(),
            config.target.secret_key.clone(),
        );

        Self {
            config,
            generator: ObjectGenerator::new("test-objects"),
            uploaded_keys: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            signer,
        }
    }
}

pub async fn put_object(user: &mut GooseUser) -> TransactionResult {
    let state_guard = crate::GLOBAL_STATE.read().await;
    let state = state_guard.as_ref().expect("Global state not initialized");

    let size = state.config.workload.object_sizes.sample();
    let key = state.generator.random_key();
    let data = state.generator.random_data(size);
    let content_hash = sha256_hex(&data);

    let uri = format!("/{}/{}", state.config.target.bucket, key);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let host = state
        .config
        .target
        .endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let headers = [
        ("host", host),
        ("x-amz-content-sha256", content_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth = state
        .signer
        .sign_request("PUT", &uri, "", &headers, &content_hash);

    let request_builder = user
        .get_request_builder(&GooseMethod::Put, &uri)?
        .header("Host", host)
        .header("x-amz-content-sha256", &content_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth)
        .header("Content-Length", size.to_string())
        .body(data);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let _response = user.request(goose_request).await?;

    state.uploaded_keys.lock().await.push(key);
    drop(state_guard);

    Ok(())
}

pub async fn get_object(user: &mut GooseUser) -> TransactionResult {
    let state_guard = crate::GLOBAL_STATE.read().await;
    let state = state_guard.as_ref().expect("Global state not initialized");

    let keys = state.uploaded_keys.lock().await;
    if keys.is_empty() {
        return Ok(());
    }

    use rand::Rng;
    let idx = rand::rng().random_range(0..keys.len());
    let key = keys[idx].clone();
    drop(keys);

    let uri = format!("/{}/{}", state.config.target.bucket, key);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let host = state
        .config
        .target
        .endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let empty_hash = sha256_hex(b"");
    let headers = [
        ("host", host),
        ("x-amz-content-sha256", empty_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth = state
        .signer
        .sign_request("GET", &uri, "", &headers, &empty_hash);

    let request_builder = user
        .get_request_builder(&GooseMethod::Get, &uri)?
        .header("Host", host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let _response = user.request(goose_request).await?;
    drop(state_guard);

    Ok(())
}

pub async fn delete_object(user: &mut GooseUser) -> TransactionResult {
    let state_guard = crate::GLOBAL_STATE.read().await;
    let state = state_guard.as_ref().expect("Global state not initialized");

    let key = {
        let mut keys = state.uploaded_keys.lock().await;
        if keys.is_empty() {
            return Ok(());
        }
        use rand::Rng;
        let idx = rand::rng().random_range(0..keys.len());
        keys.swap_remove(idx)
    };

    let uri = format!("/{}/{}", state.config.target.bucket, key);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let host = state
        .config
        .target
        .endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let empty_hash = sha256_hex(b"");
    let headers = [
        ("host", host),
        ("x-amz-content-sha256", empty_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth = state
        .signer
        .sign_request("DELETE", &uri, "", &headers, &empty_hash);

    let request_builder = user
        .get_request_builder(&GooseMethod::Delete, &uri)?
        .header("Host", host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let _response = user.request(goose_request).await?;
    drop(state_guard);

    Ok(())
}

pub async fn list_objects(user: &mut GooseUser) -> TransactionResult {
    let state_guard = crate::GLOBAL_STATE.read().await;
    let state = state_guard.as_ref().expect("Global state not initialized");

    let uri = format!("/{}", state.config.target.bucket);
    let query = "max-keys=1000";
    let path_with_query = format!("{}?{}", uri, query);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let host = state
        .config
        .target
        .endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let empty_hash = sha256_hex(b"");
    let headers = [
        ("host", host),
        ("x-amz-content-sha256", empty_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth = state
        .signer
        .sign_request("GET", &uri, query, &headers, &empty_hash);

    let request_builder = user
        .get_request_builder(&GooseMethod::Get, &path_with_query)?
        .header("Host", host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let _response = user.request(goose_request).await?;
    drop(state_guard);

    Ok(())
}

pub async fn head_object(user: &mut GooseUser) -> TransactionResult {
    let state_guard = crate::GLOBAL_STATE.read().await;
    let state = state_guard.as_ref().expect("Global state not initialized");

    let keys = state.uploaded_keys.lock().await;
    if keys.is_empty() {
        return Ok(());
    }

    use rand::Rng;
    let idx = rand::rng().random_range(0..keys.len());
    let key = keys[idx].clone();
    drop(keys);

    let uri = format!("/{}/{}", state.config.target.bucket, key);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let host = state
        .config
        .target
        .endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let empty_hash = sha256_hex(b"");
    let headers = [
        ("host", host),
        ("x-amz-content-sha256", empty_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth = state
        .signer
        .sign_request("HEAD", &uri, "", &headers, &empty_hash);

    let request_builder = user
        .get_request_builder(&GooseMethod::Head, &uri)?
        .header("Host", host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let _response = user.request(goose_request).await?;
    drop(state_guard);

    Ok(())
}
