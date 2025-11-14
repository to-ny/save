use crate::config::LoadTestConfig;
use crate::signing::sha256_hex;
use goose::prelude::*;

pub fn build_scenario(_config: &LoadTestConfig) -> Scenario {
    scenario!("Multipart")
        .register_transaction(transaction!(multipart_upload).set_name("MultipartUpload"))
}

async fn multipart_upload(user: &mut GooseUser) -> TransactionResult {
    let state = crate::GLOBAL_STATE
        .get()
        .expect("Global state not initialized");

    let key = state.generator.random_key();
    let part_size = 5 * 1024 * 1024; // 5MB per part
    let num_parts = 3;

    // Step 1: Initiate multipart upload
    let initiate_uri = format!("/{}/{}?uploads", state.config.target.bucket, key);
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
        .sign_request("POST", &initiate_uri, "uploads", &headers, &empty_hash);

    let request_builder = user
        .get_request_builder(&GooseMethod::Post, &initiate_uri)?
        .header("Host", host)
        .header("x-amz-content-sha256", &empty_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth);

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let response = user.request(goose_request).await?;

    let body = match response.response {
        Ok(r) => r.text().await.unwrap_or_default(),
        Err(_) => String::new(),
    };

    // Extract upload ID from XML response
    let upload_id = match extract_upload_id(&body) {
        Some(id) => id,
        None => return Ok(()), // Skip if upload ID not found
    };

    // Step 2: Upload parts
    let mut etags = Vec::new();

    for part_num in 1..=num_parts {
        let data = state.generator.random_data(part_size);
        let content_hash = sha256_hex(&data);

        let part_uri = format!("/{}/{}", state.config.target.bucket, key);
        let part_query = format!("uploadId={}&partNumber={}", upload_id, part_num);
        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();

        let headers = [
            ("host", host),
            ("x-amz-content-sha256", content_hash.as_str()),
            ("x-amz-date", amz_date.as_str()),
        ];

        let auth =
            state
                .signer
                .sign_request("PUT", &part_uri, &part_query, &headers, &content_hash);

        let uri_with_query = format!("{}?{}", part_uri, part_query);

        let request_builder = user
            .get_request_builder(&GooseMethod::Put, &uri_with_query)?
            .header("Host", host)
            .header("x-amz-content-sha256", &content_hash)
            .header("x-amz-date", &amz_date)
            .header("Authorization", &auth)
            .header("Content-Length", part_size.to_string())
            .body(data);

        let goose_request = GooseRequest::builder()
            .set_request_builder(request_builder)
            .build();

        let response = user.request(goose_request).await?;
        if let Ok(resp) = response.response
            && let Some(etag) = resp.headers().get("ETag")
        {
            etags.push((part_num, etag.to_str().unwrap_or("").to_string()));
        }
    }

    // Step 3: Complete multipart upload
    let complete_body = build_complete_multipart_xml(&etags);
    let complete_uri = format!("/{}/{}", state.config.target.bucket, key);
    let complete_query = format!("uploadId={}", upload_id);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let body_hash = sha256_hex(complete_body.as_bytes());

    let headers = [
        ("host", host),
        ("x-amz-content-sha256", body_hash.as_str()),
        ("x-amz-date", amz_date.as_str()),
    ];

    let auth =
        state
            .signer
            .sign_request("POST", &complete_uri, &complete_query, &headers, &body_hash);

    let uri_with_query = format!("{}?{}", complete_uri, complete_query);

    let request_builder = user
        .get_request_builder(&GooseMethod::Post, &uri_with_query)?
        .header("Host", host)
        .header("x-amz-content-sha256", &body_hash)
        .header("x-amz-date", &amz_date)
        .header("Authorization", &auth)
        .header("Content-Type", "application/xml")
        .header("Content-Length", complete_body.len().to_string())
        .body(complete_body.into_bytes());

    let goose_request = GooseRequest::builder()
        .set_request_builder(request_builder)
        .build();

    let _response = user.request(goose_request).await?;

    Ok(())
}

fn extract_upload_id(xml: &str) -> Option<String> {
    xml.split("<UploadId>")
        .nth(1)?
        .split("</UploadId>")
        .next()
        .map(|s| s.to_string())
}

fn build_complete_multipart_xml(etags: &[(u32, String)]) -> String {
    let mut xml = String::from("<CompleteMultipartUpload>");
    for (part_num, etag) in etags {
        xml.push_str(&format!(
            "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>",
            part_num, etag
        ));
    }
    xml.push_str("</CompleteMultipartUpload>");
    xml
}
