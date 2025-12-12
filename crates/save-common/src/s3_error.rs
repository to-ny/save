//! S3-compatible XML error responses.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename = "Error")]
pub struct S3Error {
    #[serde(rename = "Code")]
    pub code: S3ErrorCode,
    #[serde(rename = "Message")]
    pub message: String,
    #[serde(rename = "Resource")]
    pub resource: String,
    #[serde(rename = "RequestId")]
    pub request_id: String,
}

#[derive(Debug, Clone)]
pub enum S3ErrorCode {
    AccessDenied,
    BucketAlreadyExists,
    BucketNotEmpty,
    InternalError,
    InvalidRequest,
    NoSuchBucket,
    NoSuchKey,
    RequestTimeTooSkewed,
    ServiceUnavailable,
    SignatureDoesNotMatch,
}

impl Serialize for S3ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl std::fmt::Display for S3ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            S3ErrorCode::AccessDenied => write!(f, "AccessDenied"),
            S3ErrorCode::BucketAlreadyExists => write!(f, "BucketAlreadyExists"),
            S3ErrorCode::BucketNotEmpty => write!(f, "BucketNotEmpty"),
            S3ErrorCode::InternalError => write!(f, "InternalError"),
            S3ErrorCode::InvalidRequest => write!(f, "InvalidRequest"),
            S3ErrorCode::NoSuchBucket => write!(f, "NoSuchBucket"),
            S3ErrorCode::NoSuchKey => write!(f, "NoSuchKey"),
            S3ErrorCode::RequestTimeTooSkewed => write!(f, "RequestTimeTooSkewed"),
            S3ErrorCode::ServiceUnavailable => write!(f, "ServiceUnavailable"),
            S3ErrorCode::SignatureDoesNotMatch => write!(f, "SignatureDoesNotMatch"),
        }
    }
}

impl S3Error {
    pub fn new(code: S3ErrorCode, message: impl Into<String>, resource: impl Into<String>) -> Self {
        let request_id = generate_request_id();
        Self {
            code,
            message: message.into(),
            resource: resource.into(),
            request_id,
        }
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = request_id.into();
        self
    }

    pub fn to_xml(&self) -> String {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&quick_xml::se::to_string(self).unwrap_or_else(|_| {
            "<Error><Code>InternalError</Code><Message>Failed to serialize error</Message></Error>".to_string()
        }));
        xml
    }

    pub fn access_denied(resource: impl Into<String>) -> Self {
        Self::new(S3ErrorCode::AccessDenied, "Access Denied", resource)
    }

    pub fn bucket_already_exists(bucket: impl Into<String>) -> Self {
        let bucket_str = bucket.into();
        Self::new(
            S3ErrorCode::BucketAlreadyExists,
            format!("The requested bucket name '{}' already exists", bucket_str),
            format!("/{}", bucket_str),
        )
    }

    pub fn bucket_not_empty(bucket: impl Into<String>) -> Self {
        let bucket_str = bucket.into();
        Self::new(
            S3ErrorCode::BucketNotEmpty,
            "The bucket you tried to delete is not empty",
            format!("/{}", bucket_str),
        )
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(S3ErrorCode::InternalError, message, "/")
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(S3ErrorCode::InvalidRequest, message, "/")
    }

    pub fn no_such_bucket(bucket: impl Into<String>) -> Self {
        let bucket_str = bucket.into();
        Self::new(
            S3ErrorCode::NoSuchBucket,
            "The specified bucket does not exist",
            format!("/{}", bucket_str),
        )
    }

    pub fn no_such_key(bucket: impl Into<String>, key: impl Into<String>) -> Self {
        let bucket_str = bucket.into();
        let key_str = key.into();
        Self::new(
            S3ErrorCode::NoSuchKey,
            "The specified key does not exist",
            format!("/{}/{}", bucket_str, key_str),
        )
    }

    pub fn request_time_too_skewed() -> Self {
        Self::new(
            S3ErrorCode::RequestTimeTooSkewed,
            "The difference between the request time and the server's time is too large",
            "/",
        )
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(S3ErrorCode::ServiceUnavailable, message, "/")
    }

    pub fn signature_does_not_match() -> Self {
        Self::new(
            S3ErrorCode::SignatureDoesNotMatch,
            "The request signature we calculated does not match the signature you provided",
            "/",
        )
    }
}

fn generate_request_id() -> String {
    let uuid = uuid::Uuid::new_v4();
    let hex = uuid.as_simple().to_string();
    hex[..16].to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s3_error_to_xml() {
        let error = S3Error::new(
            S3ErrorCode::NoSuchBucket,
            "The specified bucket does not exist",
            "/test-bucket",
        )
        .with_request_id("12345ABC");

        let xml = error.to_xml();

        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<Code>NoSuchBucket</Code>"));
        assert!(xml.contains("<Message>The specified bucket does not exist</Message>"));
        assert!(xml.contains("<Resource>/test-bucket</Resource>"));
        assert!(xml.contains("<RequestId>12345ABC</RequestId>"));
    }

    #[test]
    fn test_bucket_already_exists() {
        let error = S3Error::bucket_already_exists("my-bucket");
        let xml = error.to_xml();

        assert!(xml.contains("<Code>BucketAlreadyExists</Code>"));
        assert!(xml.contains("<Resource>/my-bucket</Resource>"));
    }

    #[test]
    fn test_no_such_key() {
        let error = S3Error::no_such_key("my-bucket", "my-key.txt");
        let xml = error.to_xml();

        assert!(xml.contains("<Code>NoSuchKey</Code>"));
        assert!(xml.contains("<Resource>/my-bucket/my-key.txt</Resource>"));
    }

    #[test]
    fn test_request_id_generation() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();

        assert_eq!(id1.len(), 16, "Request ID should be 16 characters");
        assert_eq!(id2.len(), 16, "Request ID should be 16 characters");
        assert!(
            id1.chars().all(|c| c.is_ascii_hexdigit()),
            "Request ID should be hex"
        );
        assert!(
            id2.chars().all(|c| c.is_ascii_hexdigit()),
            "Request ID should be hex"
        );
        assert_ne!(id1, id2, "Request IDs should be unique");
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(S3ErrorCode::NoSuchBucket.to_string(), "NoSuchBucket");
        assert_eq!(S3ErrorCode::AccessDenied.to_string(), "AccessDenied");
        assert_eq!(S3ErrorCode::InvalidRequest.to_string(), "InvalidRequest");
    }
}
