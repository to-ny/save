//! Common utilities for concurrency tests.

#![allow(dead_code)]

use aws_config::BehaviorVersion;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Credentials, Region};
use uuid::Uuid;

/// Get the access key for testing
pub fn access_key() -> String {
    std::env::var("AWS_ACCESS_KEY_ID").unwrap_or_else(|_| "test-access-key".to_string())
}

/// Get the secret key for testing
pub fn secret_key() -> String {
    std::env::var("AWS_SECRET_ACCESS_KEY").unwrap_or_else(|_| "test-secret-key".to_string())
}

/// Get the endpoint URL for testing
pub fn endpoint() -> String {
    std::env::var("S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".to_string())
}

/// Generate a unique bucket name for testing
pub fn unique_bucket_name(prefix: &str) -> String {
    format!("{}-{}", prefix, &Uuid::new_v4().simple().to_string()[..16])
}

/// Generate a unique object key for testing
pub fn unique_key(prefix: &str) -> String {
    format!("{}-{}", prefix, Uuid::new_v4().simple())
}

/// Create AWS S3 client configured for local save-api server.
pub async fn create_client() -> Client {
    create_client_with_credentials(access_key(), secret_key()).await
}

/// Create AWS S3 client with custom credentials.
pub async fn create_client_with_credentials(access_key: String, secret_key: String) -> Client {
    let endpoint = endpoint();

    let credentials = Credentials::new(
        access_key, secret_key, None, // No session token
        None, // No expiration
        "static",
    );

    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&config)
        .endpoint_url(endpoint)
        .force_path_style(true) // Required for localhost endpoints
        .build();

    Client::from_conf(s3_config)
}

/// Delete bucket and all its contents. Idempotent.
pub async fn cleanup_bucket(client: &Client, bucket: &str) -> anyhow::Result<()> {
    // List and delete all objects
    match client.list_objects_v2().bucket(bucket).send().await {
        Ok(response) => {
            if let Some(contents) = response.contents {
                for object in contents {
                    if let Some(key) = object.key {
                        let _ = client.delete_object().bucket(bucket).key(&key).send().await;
                    }
                }
            }
        }
        Err(_) => {
            // Bucket might not exist, that's fine
            return Ok(());
        }
    }

    // List and abort all multipart uploads
    match client.list_multipart_uploads().bucket(bucket).send().await {
        Ok(response) => {
            if let Some(uploads) = response.uploads {
                for upload in uploads {
                    if let (Some(key), Some(upload_id)) = (upload.key, upload.upload_id) {
                        let _ = client
                            .abort_multipart_upload()
                            .bucket(bucket)
                            .key(&key)
                            .upload_id(&upload_id)
                            .send()
                            .await;
                    }
                }
            }
        }
        Err(_) => {
            // Ignore errors
        }
    }

    // Delete the bucket
    let _ = client.delete_bucket().bucket(bucket).send().await;

    Ok(())
}

/// Create a bucket if it doesn't exist.
///
/// Returns Ok(()) whether the bucket was created or already existed.
#[allow(dead_code)]
pub async fn ensure_bucket(client: &Client, bucket: &str) -> anyhow::Result<()> {
    match client.create_bucket().bucket(bucket).send().await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Check if error is BucketAlreadyExists or BucketAlreadyOwnedByYou
            if e.to_string().contains("BucketAlready") {
                Ok(())
            } else {
                Err(e.into())
            }
        }
    }
}

/// Compute MD5 hash of data
pub fn compute_md5(data: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(data);
    format!("{:x}", digest)
}

/// Generate random data of specified size
pub fn random_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}
