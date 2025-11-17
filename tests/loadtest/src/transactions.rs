use crate::config::LoadTestConfig;
use crate::objects::ObjectGenerator;
use crate::signing::{S3Signer, sha256_hex};
use goose::prelude::*;
use std::sync::Arc;
use url::Url;

pub struct UserState {
    pub config: LoadTestConfig,
    pub generator: ObjectGenerator,
    pub uploaded_keys: Arc<tokio::sync::Mutex<Vec<String>>>,
    pub signer: S3Signer,
}

impl UserState {
    pub fn new(config: LoadTestConfig, shared_keys: Arc<tokio::sync::Mutex<Vec<String>>>) -> Self {
        let signer = S3Signer::new(
            config.target.access_key.clone(),
            config.target.secret_key.clone(),
        );

        Self {
            config,
            generator: ObjectGenerator::new("test-objects"),
            uploaded_keys: shared_keys,
            signer,
        }
    }
}

pub fn setup_user(
    config: LoadTestConfig,
    shared_keys: Arc<tokio::sync::Mutex<Vec<String>>>,
) -> impl Fn(
    &mut GooseUser,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = TransactionResult> + Send + '_>> {
    move |user: &mut GooseUser| {
        let config = config.clone();
        let shared_keys = shared_keys.clone();
        Box::pin(async move {
            let user_state = UserState::new(config, shared_keys);
            user.set_session_data(user_state);
            Ok(())
        })
    }
}

fn extract_host(endpoint: &Url) -> String {
    let host = endpoint.host_str().unwrap_or("localhost");
    if let Some(port) = endpoint.port() {
        format!("{}:{}", host, port)
    } else {
        host.to_string()
    }
}

pub async fn put_object(user: &mut GooseUser) -> TransactionResult {
    let (size, key, data, content_hash, uri, host, amz_date, auth, uploaded_keys) = {
        let state = user.get_session_data_unchecked::<UserState>();

        let size = state.config.workload.object_sizes.sample();
        let key = state.generator.random_key();
        let data = state.generator.random_data(size);
        let content_hash = sha256_hex(&data);

        let uri = format!("/{}/{}", state.config.target.bucket, key);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = extract_host(&state.config.target.endpoint);

        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", content_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = state
            .signer
            .sign_request("PUT", &uri, "", &headers, &content_hash);

        let uploaded_keys = state.uploaded_keys.clone();

        (
            size,
            key,
            data,
            content_hash,
            uri,
            host,
            amz_date,
            auth,
            uploaded_keys,
        )
    };

    let request_builder = user
        .get_request_builder(&GooseMethod::Put, &uri)?
        .header("Host", &host)
        .header("x-amz-content-sha256", &content_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth)
        .header("Content-Length", size.to_string())
        .body(data);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(200)
        .build();

    let _response = user.request(goose_request).await?;

    uploaded_keys.lock().await.push(key);

    Ok(())
}

pub async fn get_object(user: &mut GooseUser) -> TransactionResult {
    let (_key, uri, host, auth, empty_hash, amz_date) = {
        let state = user.get_session_data_unchecked::<UserState>();

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
        let host = extract_host(&state.config.target.endpoint);

        let empty_hash = sha256_hex(b"");
        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = state
            .signer
            .sign_request("GET", &uri, "", &headers, &empty_hash);

        (key, uri, host, auth, empty_hash, amz_date)
    };

    let request_builder = user
        .get_request_builder(&GooseMethod::Get, &uri)?
        .header("Host", &host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(200)
        .build();

    let _response = user.request(goose_request).await?;

    Ok(())
}

pub async fn delete_object(user: &mut GooseUser) -> TransactionResult {
    let (_key, uri, host, auth, empty_hash, amz_date) = {
        let state = user.get_session_data_unchecked::<UserState>();

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
        let host = extract_host(&state.config.target.endpoint);

        let empty_hash = sha256_hex(b"");
        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = state
            .signer
            .sign_request("DELETE", &uri, "", &headers, &empty_hash);

        (key, uri, host, auth, empty_hash, amz_date)
    };

    let request_builder = user
        .get_request_builder(&GooseMethod::Delete, &uri)?
        .header("Host", &host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(204)
        .build();

    let _response = user.request(goose_request).await?;

    Ok(())
}

pub async fn list_objects(user: &mut GooseUser) -> TransactionResult {
    let (_uri, path_with_query, host, auth, empty_hash, amz_date) = {
        let state = user.get_session_data_unchecked::<UserState>();

        let uri = format!("/{}", state.config.target.bucket);
        let query = "max-keys=1000";
        let path_with_query = format!("{}?{}", uri, query);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = extract_host(&state.config.target.endpoint);

        let empty_hash = sha256_hex(b"");
        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = state
            .signer
            .sign_request("GET", &uri, query, &headers, &empty_hash);

        (uri, path_with_query, host, auth, empty_hash, amz_date)
    };

    let request_builder = user
        .get_request_builder(&GooseMethod::Get, &path_with_query)?
        .header("Host", &host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(200)
        .build();

    let _response = user.request(goose_request).await?;

    Ok(())
}

pub async fn head_object(user: &mut GooseUser) -> TransactionResult {
    let (_key, uri, host, auth, empty_hash, amz_date) = {
        let state = user.get_session_data_unchecked::<UserState>();

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
        let host = extract_host(&state.config.target.endpoint);

        let empty_hash = sha256_hex(b"");
        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = state
            .signer
            .sign_request("HEAD", &uri, "", &headers, &empty_hash);

        (key, uri, host, auth, empty_hash, amz_date)
    };

    let request_builder = user
        .get_request_builder(&GooseMethod::Head, &uri)?
        .header("Host", &host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .expect_status_code(200)
        .build();

    let _response = user.request(goose_request).await?;

    Ok(())
}
