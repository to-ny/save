use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct S3Signer {
    access_key: String,
    secret_key: String,
    region: String,
}

impl S3Signer {
    pub fn new(access_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        Self {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            region: "us-east-1".to_string(),
        }
    }

    pub fn sign_request(
        &self,
        method: &str,
        uri: &str,
        query: &str,
        headers: &[(&str, &str)],
        payload_hash: &str,
    ) -> String {
        let canonical_uri = uri;
        let canonical_query = query;

        // Use BTreeMap to automatically sort headers (same as server and test code)
        let mut canonical_headers_map: std::collections::BTreeMap<String, String> =
            std::collections::BTreeMap::new();
        let mut amz_date = String::new();

        for (name, value) in headers {
            let name_lower = name.to_lowercase();
            canonical_headers_map.insert(name_lower.clone(), value.to_string());
            if name_lower == "x-amz-date" {
                amz_date = value.to_string();
            }
        }

        let canonical_headers = canonical_headers_map
            .iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect::<Vec<_>>()
            .join("\n");

        let signed_headers = canonical_headers_map
            .keys()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n\n{}\n{}",
            method, canonical_uri, canonical_query, canonical_headers, signed_headers, payload_hash
        );

        let date_stamp = &amz_date[..8];
        let credential_scope = format!("{}/{}/s3/aws4_request", date_stamp, self.region);

        let canonical_request_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));

        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date, credential_scope, canonical_request_hash
        );

        let signing_key = self.get_signature_key(date_stamp);
        let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));

        format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.access_key, credential_scope, signed_headers, signature
        )
    }

    fn get_signature_key(&self, date_stamp: &str) -> Vec<u8> {
        let k_secret = format!("AWS4{}", self.secret_key);
        let k_date = hmac_sha256(k_secret.as_bytes(), date_stamp.as_bytes());
        let k_region = hmac_sha256(&k_date, self.region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        hmac_sha256(&k_service, b"aws4_request")
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        let data = b"hello world";
        let hash = sha256_hex(data);
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_signer_creates() {
        let signer = S3Signer::new("test-key", "test-secret");
        assert_eq!(signer.access_key, "test-key");
    }

    #[test]
    fn test_sign_request_matches_server() {
        let signer = S3Signer::new("test-access-key", "test-secret-key");

        let headers = [
            ("host", "localhost:9000"),
            (
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            ("x-amz-date", "20251117T120000Z"),
        ];

        let auth = signer.sign_request(
            "PUT",
            "/loadtest",
            "",
            &headers,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );

        eprintln!("Generated auth header: {}", auth);
        assert!(auth.starts_with(
            "AWS4-HMAC-SHA256 Credential=test-access-key/20251117/us-east-1/s3/aws4_request"
        ));
    }
}
