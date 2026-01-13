//! S3 operations for the traffic simulator.

use aws_sdk_s3::Client;
use rand::Rng;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::config::SizeWeight;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationType {
    Put,
    Get,
    Delete,
    List,
    Head,
    ListBuckets,
    MultipartUpload,
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OperationType::Put => write!(f, "PUT"),
            OperationType::Get => write!(f, "GET"),
            OperationType::Delete => write!(f, "DELETE"),
            OperationType::List => write!(f, "LIST"),
            OperationType::Head => write!(f, "HEAD"),
            OperationType::ListBuckets => write!(f, "LIST_BUCKETS"),
            OperationType::MultipartUpload => write!(f, "MULTIPART_UPLOAD"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OperationResult {
    pub operation: OperationType,
    pub success: bool,
    pub duration: Duration,
    pub size_bytes: Option<u64>,
    pub error: Option<String>,
}

pub struct Operations {
    client: Client,
    bucket: String,
    key_prefix: String,
    size_distribution: Vec<SizeWeight>,
}

impl Operations {
    pub fn new(
        client: Client,
        bucket: String,
        key_prefix: String,
        size_distribution: Vec<SizeWeight>,
    ) -> Self {
        Self {
            client,
            bucket,
            key_prefix,
            size_distribution,
        }
    }

    pub fn random_size(&self) -> u64 {
        let mut rng = rand::rng();
        let total_weight: u32 = self.size_distribution.iter().map(|s| s.weight).sum();
        let mut roll = rng.random_range(0..total_weight);

        for size_weight in &self.size_distribution {
            if roll < size_weight.weight {
                return rng.random_range(size_weight.min_bytes..=size_weight.max_bytes);
            }
            roll -= size_weight.weight;
        }

        // Fallback to smallest size
        1024
    }

    pub fn random_key(&self) -> String {
        let mut rng = rand::rng();
        let id: u64 = rng.random();
        format!("{}{:016x}", self.key_prefix, id)
    }

    fn random_data(size: u64) -> Vec<u8> {
        use rand::Rng;
        let mut rng = rand::rng();
        let mut data = vec![0u8; size as usize];
        rng.fill(&mut data[..]);
        data
    }

    pub async fn put_object(&self, key: &str, size: u64) -> OperationResult {
        let start = Instant::now();
        let data = Self::random_data(size);

        let result = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.into())
            .send()
            .await;

        let duration = start.elapsed();
        match result {
            Ok(_) => {
                info!(
                    operation = "PUT",
                    key = key,
                    size_bytes = size,
                    duration_ms = duration.as_millis(),
                    "Object uploaded"
                );
                OperationResult {
                    operation: OperationType::Put,
                    success: true,
                    duration,
                    size_bytes: Some(size),
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "PUT",
                    key = key,
                    error = %e,
                    "PUT failed"
                );
                OperationResult {
                    operation: OperationType::Put,
                    success: false,
                    duration,
                    size_bytes: Some(size),
                    error: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn get_object(&self, key: &str) -> OperationResult {
        let start = Instant::now();

        let result = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        let duration = start.elapsed();
        match result {
            Ok(response) => {
                // Read body to measure full transfer time
                let body_result = response.body.collect().await;
                let size = body_result.map(|b| b.into_bytes().len() as u64).ok();

                info!(
                    operation = "GET",
                    key = key,
                    size_bytes = ?size,
                    duration_ms = duration.as_millis(),
                    "Object downloaded"
                );
                OperationResult {
                    operation: OperationType::Get,
                    success: true,
                    duration: start.elapsed(),
                    size_bytes: size,
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "GET",
                    key = key,
                    error = %e,
                    "GET failed"
                );
                OperationResult {
                    operation: OperationType::Get,
                    success: false,
                    duration,
                    size_bytes: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn delete_object(&self, key: &str) -> OperationResult {
        let start = Instant::now();

        let result = self
            .client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        let duration = start.elapsed();
        match result {
            Ok(_) => {
                info!(
                    operation = "DELETE",
                    key = key,
                    duration_ms = duration.as_millis(),
                    "Object deleted"
                );
                OperationResult {
                    operation: OperationType::Delete,
                    success: true,
                    duration,
                    size_bytes: None,
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "DELETE",
                    key = key,
                    error = %e,
                    "DELETE failed"
                );
                OperationResult {
                    operation: OperationType::Delete,
                    success: false,
                    duration,
                    size_bytes: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn head_object(&self, key: &str) -> OperationResult {
        let start = Instant::now();

        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        let duration = start.elapsed();
        match result {
            Ok(response) => {
                let size = response.content_length().map(|l| l as u64);
                info!(
                    operation = "HEAD",
                    key = key,
                    size_bytes = ?size,
                    duration_ms = duration.as_millis(),
                    "HEAD completed"
                );
                OperationResult {
                    operation: OperationType::Head,
                    success: true,
                    duration,
                    size_bytes: size,
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "HEAD",
                    key = key,
                    error = %e,
                    "HEAD failed"
                );
                OperationResult {
                    operation: OperationType::Head,
                    success: false,
                    duration,
                    size_bytes: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn list_objects(&self, prefix: Option<&str>) -> OperationResult {
        let start = Instant::now();

        let mut request = self.client.list_objects_v2().bucket(&self.bucket);
        if let Some(p) = prefix {
            request = request.prefix(p);
        }

        let result = request.send().await;

        let duration = start.elapsed();
        match result {
            Ok(response) => {
                let count = response.key_count().unwrap_or(0);
                info!(
                    operation = "LIST",
                    prefix = ?prefix,
                    object_count = count,
                    duration_ms = duration.as_millis(),
                    "LIST completed"
                );
                OperationResult {
                    operation: OperationType::List,
                    success: true,
                    duration,
                    size_bytes: None,
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "LIST",
                    prefix = ?prefix,
                    error = %e,
                    "LIST failed"
                );
                OperationResult {
                    operation: OperationType::List,
                    success: false,
                    duration,
                    size_bytes: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn list_buckets(&self) -> OperationResult {
        let start = Instant::now();

        let result = self.client.list_buckets().send().await;

        let duration = start.elapsed();
        match result {
            Ok(response) => {
                let count = response.buckets().len();
                info!(
                    operation = "LIST_BUCKETS",
                    bucket_count = count,
                    duration_ms = duration.as_millis(),
                    "LIST_BUCKETS completed"
                );
                OperationResult {
                    operation: OperationType::ListBuckets,
                    success: true,
                    duration,
                    size_bytes: None,
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "LIST_BUCKETS",
                    error = %e,
                    "LIST_BUCKETS failed"
                );
                OperationResult {
                    operation: OperationType::ListBuckets,
                    success: false,
                    duration,
                    size_bytes: None,
                    error: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn multipart_upload(&self, key: &str, size: u64) -> OperationResult {
        let start = Instant::now();
        let part_size: u64 = 5 * 1024 * 1024; // 5MB minimum part size
        let num_parts = size.div_ceil(part_size) as i32;

        // Create multipart upload
        let create_result = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await;

        let upload_id = match create_result {
            Ok(response) => match response.upload_id() {
                Some(id) => id.to_string(),
                None => {
                    return OperationResult {
                        operation: OperationType::MultipartUpload,
                        success: false,
                        duration: start.elapsed(),
                        size_bytes: Some(size),
                        error: Some("No upload ID returned".to_string()),
                    };
                }
            },
            Err(e) => {
                return OperationResult {
                    operation: OperationType::MultipartUpload,
                    success: false,
                    duration: start.elapsed(),
                    size_bytes: Some(size),
                    error: Some(format!("Failed to create multipart upload: {}", e)),
                };
            }
        };

        // Upload parts
        let mut completed_parts = Vec::new();
        let mut remaining = size;

        for part_number in 1..=num_parts {
            let this_part_size = remaining.min(part_size);
            let data = Self::random_data(this_part_size);

            let upload_part_result = self
                .client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(data.into())
                .send()
                .await;

            match upload_part_result {
                Ok(response) => {
                    if let Some(etag) = response.e_tag() {
                        completed_parts.push(
                            aws_sdk_s3::types::CompletedPart::builder()
                                .e_tag(etag)
                                .part_number(part_number)
                                .build(),
                        );
                    }
                }
                Err(e) => {
                    // Abort the upload on failure
                    let _ = self
                        .client
                        .abort_multipart_upload()
                        .bucket(&self.bucket)
                        .key(key)
                        .upload_id(&upload_id)
                        .send()
                        .await;

                    return OperationResult {
                        operation: OperationType::MultipartUpload,
                        success: false,
                        duration: start.elapsed(),
                        size_bytes: Some(size),
                        error: Some(format!("Failed to upload part {}: {}", part_number, e)),
                    };
                }
            }

            remaining -= this_part_size;
        }

        // Complete multipart upload
        let complete_result = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .set_parts(Some(completed_parts))
                    .build(),
            )
            .send()
            .await;

        let duration = start.elapsed();
        match complete_result {
            Ok(_) => {
                info!(
                    operation = "MULTIPART_UPLOAD",
                    key = key,
                    size_bytes = size,
                    parts = num_parts,
                    duration_ms = duration.as_millis(),
                    "Multipart upload completed"
                );
                OperationResult {
                    operation: OperationType::MultipartUpload,
                    success: true,
                    duration,
                    size_bytes: Some(size),
                    error: None,
                }
            }
            Err(e) => {
                warn!(
                    operation = "MULTIPART_UPLOAD",
                    key = key,
                    error = %e,
                    "Multipart upload failed"
                );
                OperationResult {
                    operation: OperationType::MultipartUpload,
                    success: false,
                    duration,
                    size_bytes: Some(size),
                    error: Some(e.to_string()),
                }
            }
        }
    }
}
