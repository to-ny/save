use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use tracing::{debug, warn};

use crate::state::AppState;

pub async fn validate_sigv4(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(auth) => {
            if !auth.starts_with("AWS4-HMAC-SHA256") {
                warn!("Invalid authorization header format");
                return Err(StatusCode::UNAUTHORIZED);
            }

            let access_key = extract_access_key(auth);
            match access_key {
                Some(key) if key == state.config.credentials.access_key => {
                    debug!("SigV4 validation passed for access key: {}", key);
                    Ok(next.run(request).await)
                }
                Some(key) => {
                    warn!("Access key mismatch: {}", key);
                    Err(StatusCode::UNAUTHORIZED)
                }
                None => {
                    warn!("Could not extract access key from authorization header");
                    Err(StatusCode::UNAUTHORIZED)
                }
            }
        }
        None => {
            warn!("Missing authorization header");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

fn extract_access_key(auth_header: &str) -> Option<String> {
    auth_header
        .split("Credential=")
        .nth(1)?
        .split('/')
        .next()
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_access_key() {
        let header = "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20230101/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abcd1234";
        let key = extract_access_key(header);
        assert_eq!(key, Some("AKIAIOSFODNN7EXAMPLE".to_string()));
    }

    #[test]
    fn test_extract_access_key_invalid() {
        let header = "AWS4-HMAC-SHA256";
        let key = extract_access_key(header);
        assert_eq!(key, None);
    }
}
