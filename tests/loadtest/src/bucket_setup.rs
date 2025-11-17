use crate::config::LoadTestConfig;
use crate::signing::{S3Signer, sha256_hex};
use anyhow::Result;

/// Ensure the test bucket exists, creating it if necessary
pub async fn ensure_bucket_exists(config: &LoadTestConfig) -> Result<()> {
    let client = reqwest::Client::new();
    let signer = S3Signer::new(
        config.target.access_key.clone(),
        config.target.secret_key.clone(),
    );

    let endpoint = &config.target.endpoint;
    let bucket = &config.target.bucket;
    // Ensure endpoint doesn't end with slash to avoid double slashes
    let url = format!("{}/{}", endpoint.as_str().trim_end_matches('/'), bucket);

    let host = endpoint
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid endpoint: no host"))?;
    let host = if let Some(port) = endpoint.port() {
        format!("{}:{}", host, port)
    } else {
        host.to_string()
    };

    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let empty_hash = sha256_hex(b"");

    let headers = [
        ("host", host.as_str()),
        ("x-amz-content-sha256", empty_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let uri = format!("/{}", bucket);
    let auth = signer.sign_request("HEAD", &uri, "", &headers, &empty_hash);

    let response = client
        .head(&url)
        .header("Host", &host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth)
        .send()
        .await?;

    if response.status() == 200 {
        return Ok(());
    }

    println!("Creating test bucket: {}", bucket);

    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let empty_hash = sha256_hex(b"");

    let headers = [
        ("host", host.as_str()),
        ("x-amz-content-sha256", empty_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth = signer.sign_request("PUT", &uri, "", &headers, &empty_hash);

    let response = client
        .put(&url)
        .header("Host", &host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth)
        .send()
        .await?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to create bucket: {} - {}",
            response.status(),
            response.text().await?
        );
    }

    println!("Bucket created successfully");
    Ok(())
}
