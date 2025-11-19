use crate::config::LoadTestConfig;
use crate::signing::{S3Signer, sha256_hex};
use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use url::Url;

#[derive(Debug, Clone)]
pub struct RequestMetrics {
    pub operation: Operation,
    pub object_size: usize,
    pub end_to_end_ms: u64,
    pub server_storage_ms: Option<u64>,
    pub success: bool,
    pub error: Option<String>,
}

impl RequestMetrics {
    pub fn network_ms(&self) -> Option<u64> {
        self.server_storage_ms
            .map(|storage| self.end_to_end_ms.saturating_sub(storage))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    Put,
    Get,
    Delete,
    List,
    Head,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operation::Put => write!(f, "PUT"),
            Operation::Get => write!(f, "GET"),
            Operation::Delete => write!(f, "DELETE"),
            Operation::List => write!(f, "LIST"),
            Operation::Head => write!(f, "HEAD"),
        }
    }
}

pub struct BenchmarkRunner {
    client: Client,
    config: LoadTestConfig,
    signer: S3Signer,
    semaphore: Arc<Semaphore>,
}

impl BenchmarkRunner {
    pub fn new(config: LoadTestConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .pool_max_idle_per_host(config.workload.users.max)
            .build()
            .expect("Failed to create HTTP client");

        let signer = S3Signer::new(
            config.target.access_key.clone(),
            config.target.secret_key.clone(),
        );

        let semaphore = Arc::new(Semaphore::new(config.workload.users.max));

        Self {
            client,
            config,
            signer,
            semaphore,
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

    pub async fn put_object(&self, key: &str, data: Vec<u8>) -> crate::Result<RequestMetrics> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let size = data.len();
        let content_hash = sha256_hex(&data);
        let uri = format!("/{}/{}", self.config.target.bucket, key);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = Self::extract_host(&self.config.target.endpoint);

        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", content_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = self
            .signer
            .sign_request("PUT", &uri, "", &headers, &content_hash);

        let url = self.config.target.endpoint.join(&uri)?;

        let start = Instant::now();
        let result = self
            .client
            .put(url)
            .header("Host", &host)
            .header("x-amz-content-sha256", &content_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &auth)
            .header("Content-Length", size)
            .body(data)
            .send()
            .await;

        let end_to_end_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                let server_storage_ms = response
                    .headers()
                    .get("x-storage-duration-ms")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok());

                let success = response.status().is_success();
                let error = if !success {
                    Some(format!("HTTP {}", response.status()))
                } else {
                    None
                };

                Ok(RequestMetrics {
                    operation: Operation::Put,
                    object_size: size,
                    end_to_end_ms,
                    server_storage_ms,
                    success,
                    error,
                })
            }
            Err(e) => Ok(RequestMetrics {
                operation: Operation::Put,
                object_size: size,
                end_to_end_ms,
                server_storage_ms: None,
                success: false,
                error: Some(e.to_string()),
            }),
        }
    }

    pub async fn get_object(&self, key: &str) -> crate::Result<RequestMetrics> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let uri = format!("/{}/{}", self.config.target.bucket, key);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = Self::extract_host(&self.config.target.endpoint);
        let empty_hash = sha256_hex(b"");

        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = self
            .signer
            .sign_request("GET", &uri, "", &headers, &empty_hash);

        let url = self.config.target.endpoint.join(&uri)?;

        let start = Instant::now();
        let result = self
            .client
            .get(url)
            .header("Host", &host)
            .header("x-amz-content-sha256", &empty_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &auth)
            .send()
            .await;

        let end_to_end_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                let server_storage_ms = response
                    .headers()
                    .get("x-storage-duration-ms")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok());

                let content_length = response
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                let success = response.status().is_success();
                let error = if !success {
                    Some(format!("HTTP {}", response.status()))
                } else {
                    None
                };

                if success {
                    let _ = response.bytes().await;
                }

                Ok(RequestMetrics {
                    operation: Operation::Get,
                    object_size: content_length,
                    end_to_end_ms,
                    server_storage_ms,
                    success,
                    error,
                })
            }
            Err(e) => Ok(RequestMetrics {
                operation: Operation::Get,
                object_size: 0,
                end_to_end_ms,
                server_storage_ms: None,
                success: false,
                error: Some(e.to_string()),
            }),
        }
    }

    pub async fn delete_object(&self, key: &str) -> crate::Result<RequestMetrics> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let uri = format!("/{}/{}", self.config.target.bucket, key);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = Self::extract_host(&self.config.target.endpoint);
        let empty_hash = sha256_hex(b"");

        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = self
            .signer
            .sign_request("DELETE", &uri, "", &headers, &empty_hash);

        let url = self.config.target.endpoint.join(&uri)?;

        let start = Instant::now();
        let result = self
            .client
            .delete(url)
            .header("Host", &host)
            .header("x-amz-content-sha256", &empty_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &auth)
            .send()
            .await;

        let end_to_end_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                let success = response.status().is_success();
                let error = if !success {
                    Some(format!("HTTP {}", response.status()))
                } else {
                    None
                };

                Ok(RequestMetrics {
                    operation: Operation::Delete,
                    object_size: 0,
                    end_to_end_ms,
                    server_storage_ms: None,
                    success,
                    error,
                })
            }
            Err(e) => Ok(RequestMetrics {
                operation: Operation::Delete,
                object_size: 0,
                end_to_end_ms,
                server_storage_ms: None,
                success: false,
                error: Some(e.to_string()),
            }),
        }
    }

    pub async fn list_objects(&self) -> crate::Result<RequestMetrics> {
        let _permit = self.semaphore.acquire().await.unwrap();

        let uri = format!("/{}", self.config.target.bucket);
        let query = "max-keys=1000";
        let path_with_query = format!("{}?{}", uri, query);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let host = Self::extract_host(&self.config.target.endpoint);
        let empty_hash = sha256_hex(b"");

        let headers = [
            ("host", host.as_str()),
            ("x-amz-content-sha256", empty_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth = self
            .signer
            .sign_request("GET", &uri, query, &headers, &empty_hash);

        let url = self.config.target.endpoint.join(&path_with_query)?;

        let start = Instant::now();
        let result = self
            .client
            .get(url)
            .header("Host", &host)
            .header("x-amz-content-sha256", &empty_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &auth)
            .send()
            .await;

        let end_to_end_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(response) => {
                let success = response.status().is_success();
                let error = if !success {
                    Some(format!("HTTP {}", response.status()))
                } else {
                    None
                };

                if success {
                    let _ = response.bytes().await;
                }

                Ok(RequestMetrics {
                    operation: Operation::List,
                    object_size: 0,
                    end_to_end_ms,
                    server_storage_ms: None,
                    success,
                    error,
                })
            }
            Err(e) => Ok(RequestMetrics {
                operation: Operation::List,
                object_size: 0,
                end_to_end_ms,
                server_storage_ms: None,
                success: false,
                error: Some(e.to_string()),
            }),
        }
    }
}
