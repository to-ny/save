use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tracing::{debug, warn};

use crate::handlers::ApiError;
use crate::metrics::auth_events_total;
use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

const MAX_TIME_SKEW_SECS: i64 = 15 * 60;

/// AWS SigV4 authentication middleware
pub async fn validate_sigv4(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    debug!(
        method = %request.method(),
        uri = %request.uri(),
        query = ?request.uri().query(),
        "Incoming S3 request"
    );

    let headers = request.headers();
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            warn!(
                event = "auth_failure",
                reason = "missing_authorization_header",
                method = %request.method(),
                uri = %request.uri().path(),
                "Authentication failed: missing authorization header"
            );
            auth_events_total()
                .with_label_values(&["failure", "missing_auth"])
                .inc();
            ApiError::Unauthorized
        })?;

    if !auth_header.starts_with("AWS4-HMAC-SHA256") {
        warn!(
            event = "auth_failure",
            reason = "invalid_auth_format",
            method = %request.method(),
            uri = %request.uri().path(),
            "Authentication failed: invalid authorization format"
        );
        auth_events_total()
            .with_label_values(&["failure", "invalid_format"])
            .inc();
        return Err(ApiError::InvalidSignatureException(
            "Authorization header format is invalid".to_string(),
        ));
    }

    let auth_info = parse_authorization_header(auth_header)?;

    if auth_info.access_key != state.config.credentials.access_key {
        warn!(
            event = "auth_failure",
            reason = "access_key_mismatch",
            "Authentication failed: access key mismatch"
        );
        auth_events_total()
            .with_label_values(&["failure", "key_mismatch"])
            .inc();
        return Err(ApiError::Unauthorized);
    }

    let amz_date = headers
        .get("x-amz-date")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            warn!(
                event = "auth_failure",
                reason = "missing_amz_date_header",
                method = %request.method(),
                uri = %request.uri().path(),
                "Authentication failed: missing x-amz-date header"
            );
            auth_events_total()
                .with_label_values(&["failure", "missing_date"])
                .inc();
            ApiError::InvalidSignatureException("Missing x-amz-date header".to_string())
        })?;

    validate_timestamp(amz_date)?;

    let content_sha256 = headers
        .get("x-amz-content-sha256")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("UNSIGNED-PAYLOAD");

    let canonical_request = build_canonical_request(
        request.method().as_str(),
        request.uri().path(),
        request.uri().query().unwrap_or(""),
        headers,
        &auth_info.signed_headers,
        content_sha256,
    )?;

    let string_to_sign = build_string_to_sign(
        amz_date,
        &auth_info.scope,
        &canonical_request,
    );

    let calculated_signature = calculate_signature(
        &state.config.credentials.secret_key,
        amz_date,
        &auth_info.region,
        &auth_info.service,
        &string_to_sign,
    );

    if !constant_time_compare(&auth_info.signature, &calculated_signature) {
        warn!(
            event = "auth_failure",
            reason = "signature_mismatch",
            "Authentication failed: signature does not match"
        );
        auth_events_total()
            .with_label_values(&["failure", "signature_mismatch"])
            .inc();
        return Err(ApiError::SignatureDoesNotMatch);
    }

    debug!(
        event = "auth_success",
        method = %request.method(),
        uri = %request.uri().path(),
        "SigV4 authentication successful"
    );
    auth_events_total()
        .with_label_values(&["success", "ok"])
        .inc();

    Ok(next.run(request).await)
}

/// Parsed authorization header information
#[derive(Debug)]
struct AuthInfo {
    access_key: String,
    scope: String,
    region: String,
    service: String,
    signed_headers: Vec<String>,
    signature: String,
}

fn parse_authorization_header(auth_header: &str) -> Result<AuthInfo, ApiError> {
    // Format: AWS4-HMAC-SHA256 Credential=ACCESS_KEY/DATE/REGION/SERVICE/aws4_request, SignedHeaders=..., Signature=...
    let parts: Vec<&str> = auth_header.split(", ").collect();

    if parts.len() < 3 {
        return Err(ApiError::InvalidSignatureException(
            "Malformed authorization header".to_string(),
        ));
    }

    let credential_part = parts[0]
        .strip_prefix("AWS4-HMAC-SHA256 Credential=")
        .ok_or_else(|| {
            ApiError::InvalidSignatureException("Missing Credential in authorization".to_string())
        })?;

    let credential_components: Vec<&str> = credential_part.split('/').collect();
    if credential_components.len() != 5 {
        return Err(ApiError::InvalidSignatureException(
            "Invalid credential format".to_string(),
        ));
    }

    let access_key = credential_components[0].to_string();
    let date = credential_components[1];
    let region = credential_components[2].to_string();
    let service = credential_components[3].to_string();
    let scope = format!("{}/{}/{}/aws4_request", date, region, service);

    let signed_headers_part = parts[1]
        .strip_prefix("SignedHeaders=")
        .ok_or_else(|| {
            ApiError::InvalidSignatureException(
                "Missing SignedHeaders in authorization".to_string(),
            )
        })?;
    let signed_headers: Vec<String> = signed_headers_part
        .split(';')
        .map(|s| s.to_string())
        .collect();

    let signature = parts[2]
        .strip_prefix("Signature=")
        .ok_or_else(|| {
            ApiError::InvalidSignatureException("Missing Signature in authorization".to_string())
        })?
        .to_string();

    Ok(AuthInfo {
        access_key,
        scope,
        region,
        service,
        signed_headers,
        signature,
    })
}

fn build_canonical_request(
    method: &str,
    path: &str,
    query: &str,
    headers: &axum::http::HeaderMap,
    signed_headers: &[String],
    payload_hash: &str,
) -> Result<String, ApiError> {
    let canonical_method = method;
    let canonical_uri = if path.is_empty() { "/" } else { path };
    let canonical_query = canonicalize_query_string(query);

    let mut canonical_headers_map: BTreeMap<String, String> = BTreeMap::new();
    for header_name in signed_headers {
        if let Some(value) = headers.get(header_name)
            && let Ok(value_str) = value.to_str()
        {
            let normalized_value = value_str.trim().replace("  ", " ");
            canonical_headers_map.insert(header_name.to_lowercase(), normalized_value);
        }
    }

    let canonical_headers = canonical_headers_map
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    let canonical_signed_headers = signed_headers.join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n\n{}\n{}",
        canonical_method,
        canonical_uri,
        canonical_query,
        canonical_headers,
        canonical_signed_headers,
        payload_hash
    );

    Ok(canonical_request)
}

fn canonicalize_query_string(query: &str) -> String {
    if query.is_empty() {
        return String::new();
    }

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
}

fn build_string_to_sign(amz_date: &str, scope: &str, canonical_request: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_request.as_bytes());
    let canonical_request_hash = hex::encode(hasher.finalize());

    format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, scope, canonical_request_hash
    )
}

fn calculate_signature(
    secret_key: &str,
    amz_date: &str,
    region: &str,
    service: &str,
    string_to_sign: &str,
) -> String {
    let date = &amz_date[..8];

    let k_secret = format!("AWS4{}", secret_key);
    let k_date = hmac_sha256(k_secret.as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    let signature = hmac_sha256(&k_signing, string_to_sign.as_bytes());
    hex::encode(signature)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn validate_timestamp(amz_date: &str) -> Result<(), ApiError> {
    // Parse X-Amz-Date (format: YYYYMMDDTHHMMSSZ)
    let request_time = DateTime::parse_from_str(&format!("{}+00:00", amz_date), "%Y%m%dT%H%M%SZ%z")
        .map_err(|_| {
            ApiError::InvalidSignatureException("Invalid X-Amz-Date format".to_string())
        })?
        .with_timezone(&Utc);

    let now = Utc::now();
    let diff = (now - request_time).num_seconds().abs();

    if diff > MAX_TIME_SKEW_SECS {
        warn!(
            event = "auth_failure",
            reason = "request_time_too_skewed",
            time_diff_seconds = diff,
            max_skew_seconds = MAX_TIME_SKEW_SECS,
            "Authentication failed: request timestamp outside acceptable window"
        );
        auth_events_total()
            .with_label_values(&["failure", "time_skewed"])
            .inc();
        return Err(ApiError::RequestTimeTooSkewed);
    }

    Ok(())
}

fn constant_time_compare(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (a_byte, b_byte)| acc | (a_byte ^ b_byte))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_authorization_header() {
        let header = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20230101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abcd1234";
        let auth_info = parse_authorization_header(header).unwrap();

        assert_eq!(auth_info.access_key, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(auth_info.region, "us-east-1");
        assert_eq!(auth_info.service, "s3");
        assert_eq!(auth_info.scope, "20230101/us-east-1/s3/aws4_request");
        assert_eq!(auth_info.signed_headers, vec!["host", "x-amz-date"]);
        assert_eq!(auth_info.signature, "abcd1234");
    }

    #[test]
    fn test_parse_authorization_header_invalid() {
        let header = "AWS4-HMAC-SHA256";
        let result = parse_authorization_header(header);
        assert!(result.is_err());
    }

    #[test]
    fn test_canonicalize_query_string() {
        // Empty query
        assert_eq!(canonicalize_query_string(""), "");

        // Single parameter
        assert_eq!(canonicalize_query_string("key=value"), "key=value");

        // Multiple parameters (should be sorted)
        assert_eq!(
            canonicalize_query_string("zebra=2&alpha=1"),
            "alpha=1&zebra=2"
        );

        // Parameter with empty value
        assert_eq!(canonicalize_query_string("key="), "key=");
    }

    #[test]
    fn test_build_string_to_sign() {
        let canonical_request = "GET\n/\n\nhost:example.com\n\nhost\nUNSIGNED-PAYLOAD";
        let result = build_string_to_sign(
            "20230101T120000Z",
            "20230101/us-east-1/s3/aws4_request",
            canonical_request,
        );

        assert!(result.starts_with("AWS4-HMAC-SHA256\n"));
        assert!(result.contains("20230101T120000Z"));
        assert!(result.contains("20230101/us-east-1/s3/aws4_request"));
    }

    #[test]
    fn test_calculate_signature() {
        // Test with known values from AWS documentation
        let signature = calculate_signature(
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "20150830T123600Z",
            "us-east-1",
            "iam",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/iam/aws4_request\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );

        // The signature should be a 64-character hex string
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_validate_timestamp_valid() {
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        assert!(validate_timestamp(&amz_date).is_ok());
    }

    #[test]
    fn test_validate_timestamp_too_old() {
        let old_time = Utc::now() - chrono::Duration::seconds(MAX_TIME_SKEW_SECS + 60);
        let amz_date = old_time.format("%Y%m%dT%H%M%SZ").to_string();
        let result = validate_timestamp(&amz_date);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::RequestTimeTooSkewed));
    }

    #[test]
    fn test_validate_timestamp_too_new() {
        let future_time = Utc::now() + chrono::Duration::seconds(MAX_TIME_SKEW_SECS + 60);
        let amz_date = future_time.format("%Y%m%dT%H%M%SZ").to_string();
        let result = validate_timestamp(&amz_date);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::RequestTimeTooSkewed));
    }

    #[test]
    fn test_validate_timestamp_invalid_format() {
        let result = validate_timestamp("invalid-date");
        assert!(result.is_err());
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("abc123", "abc123"));
        assert!(!constant_time_compare("abc123", "abc124"));
        assert!(!constant_time_compare("abc123", "abc12"));
        assert!(!constant_time_compare("abc", "abcd"));
    }

    #[test]
    fn test_hmac_sha256() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let result = hmac_sha256(key, data);

        // Should produce 32 bytes (256 bits)
        assert_eq!(result.len(), 32);
    }

    #[test]
    fn test_build_canonical_request() {
        use axum::http::HeaderMap;

        let mut headers = HeaderMap::new();
        headers.insert("host", "s3.amazonaws.com".parse().unwrap());
        headers.insert("x-amz-date", "20230101T120000Z".parse().unwrap());

        let signed_headers = vec!["host".to_string(), "x-amz-date".to_string()];

        let result = build_canonical_request(
            "GET",
            "/test-bucket/test-key",
            "",
            &headers,
            &signed_headers,
            "UNSIGNED-PAYLOAD",
        )
        .unwrap();

        assert!(result.contains("GET"));
        assert!(result.contains("/test-bucket/test-key"));
        assert!(result.contains("host:s3.amazonaws.com"));
        assert!(result.contains("x-amz-date:20230101T120000Z"));
        assert!(result.contains("UNSIGNED-PAYLOAD"));
    }
}
