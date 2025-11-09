//! S3-compatible XML response types.
//!
//! This module implements the AWS S3 XML response formats for successful operations.
//! All responses follow the structure specified in the AWS S3 API documentation.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::fmt;
use tracing::error;

/// S3 XML namespace
pub const S3_XMLNS: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

/// Errors that can occur during XML response serialization.
#[derive(Debug)]
pub enum SerializationError {
    /// Failed to serialize to XML
    XmlSerialization(quick_xml::DeError),
}

impl fmt::Display for SerializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::XmlSerialization(e) => write!(f, "XML serialization failed: {}", e),
        }
    }
}

impl std::error::Error for SerializationError {}

impl From<quick_xml::DeError> for SerializationError {
    fn from(e: quick_xml::DeError) -> Self {
        Self::XmlSerialization(e)
    }
}

/// Storage class for S3 objects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub enum StorageClass {
    /// Standard storage class
    #[default]
    Standard,
}

impl StorageClass {
    /// Get the S3 API string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "STANDARD",
        }
    }
}

impl fmt::Display for StorageClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Trait for S3 XML response serialization with optimized memory allocation
pub trait S3XmlResponse: Serialize {
    /// Get the name of the root XML element for error messages
    fn root_element_name() -> &'static str;

    /// Serialize to XML string with proper error handling and logging
    fn to_xml(&self) -> Result<String, SerializationError> {
        // Pre-allocate string with reasonable capacity to reduce allocations
        let mut xml = String::with_capacity(2048);

        // Write XML declaration
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");

        // Serialize directly to string
        if let Err(e) = quick_xml::se::to_writer(&mut xml, self) {
            error!(
                element = Self::root_element_name(),
                error = %e,
                "Failed to serialize S3 XML response"
            );
            return Err(e.into());
        }

        Ok(xml)
    }
}

/// Format a DateTime for S3 XML responses (ISO 8601 format).
fn format_s3_timestamp(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// ListAllMyBucketsResult - Response for GET /
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListAllMyBucketsResult")]
pub struct ListAllMyBucketsResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Owner")]
    pub owner: Owner,
    #[serde(rename = "Buckets")]
    pub buckets: Buckets,
}

#[derive(Debug, Clone, Serialize)]
pub struct Owner {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "DisplayName")]
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Buckets {
    #[serde(rename = "Bucket")]
    pub bucket: Vec<BucketEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BucketEntry {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "CreationDate", serialize_with = "serialize_timestamp")]
    pub creation_date: DateTime<Utc>,
}

fn serialize_timestamp<S>(dt: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format_s3_timestamp(dt))
}

fn serialize_storage_class<S>(sc: &StorageClass, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(sc.as_str())
}

impl ListAllMyBucketsResult {
    /// Create a new ListAllMyBucketsResult response
    ///
    /// # Arguments
    /// * `buckets` - List of (bucket_name, creation_date) tuples
    /// * `owner_id` - Owner identifier (user/account ID)
    pub fn new(buckets: Vec<(String, DateTime<Utc>)>, owner_id: String) -> Self {
        Self {
            xmlns: S3_XMLNS.to_string(),
            owner: Owner {
                id: owner_id.clone(),
                display_name: owner_id,
            },
            buckets: Buckets {
                bucket: buckets
                    .into_iter()
                    .map(|(name, creation_date)| BucketEntry {
                        name,
                        creation_date,
                    })
                    .collect(),
            },
        }
    }
}

impl S3XmlResponse for ListAllMyBucketsResult {
    fn root_element_name() -> &'static str {
        "ListAllMyBucketsResult"
    }
}

/// ListBucketResult - Response for GET /{bucket}
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListBucketResult")]
pub struct ListBucketResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Prefix")]
    pub prefix: String,
    #[serde(rename = "Marker")]
    pub marker: String,
    #[serde(rename = "MaxKeys")]
    pub max_keys: usize,
    #[serde(rename = "IsTruncated")]
    pub is_truncated: bool,
    #[serde(rename = "Contents")]
    pub contents: Vec<ObjectEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectEntry {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "LastModified", serialize_with = "serialize_timestamp")]
    pub last_modified: DateTime<Utc>,
    #[serde(rename = "ETag")]
    pub etag: String,
    #[serde(rename = "Size")]
    pub size: u64,
    #[serde(rename = "StorageClass", serialize_with = "serialize_storage_class")]
    pub storage_class: StorageClass,
}

impl ListBucketResult {
    pub fn new(
        name: String,
        prefix: Option<String>,
        marker: Option<String>,
        max_keys: usize,
        is_truncated: bool,
        contents: Vec<(String, DateTime<Utc>, String, u64)>,
    ) -> Self {
        Self {
            xmlns: S3_XMLNS.to_string(),
            name,
            prefix: prefix.unwrap_or_default(),
            marker: marker.unwrap_or_default(),
            max_keys,
            is_truncated,
            contents: contents
                .into_iter()
                .map(|(key, last_modified, etag, size)| ObjectEntry {
                    key,
                    last_modified,
                    etag,
                    size,
                    storage_class: StorageClass::default(),
                })
                .collect(),
        }
    }
}

impl S3XmlResponse for ListBucketResult {
    fn root_element_name() -> &'static str {
        "ListBucketResult"
    }
}

/// InitiateMultipartUploadResult - Response for POST /{bucket}/{key}?uploads
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "InitiateMultipartUploadResult")]
pub struct InitiateMultipartUploadResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "UploadId")]
    pub upload_id: String,
}

impl InitiateMultipartUploadResult {
    pub fn new(bucket: String, key: String, upload_id: String) -> Self {
        Self {
            xmlns: S3_XMLNS.to_string(),
            bucket,
            key,
            upload_id,
        }
    }
}

impl S3XmlResponse for InitiateMultipartUploadResult {
    fn root_element_name() -> &'static str {
        "InitiateMultipartUploadResult"
    }
}

/// CompleteMultipartUploadResult - Response for POST /{bucket}/{key}?uploadId=...
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "CompleteMultipartUploadResult")]
pub struct CompleteMultipartUploadResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Location")]
    pub location: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "ETag")]
    pub etag: String,
}

impl CompleteMultipartUploadResult {
    pub fn new(bucket: String, key: String, etag: String, endpoint: String) -> Self {
        let location = format!("{}/{}/{}", endpoint, bucket, key);
        Self {
            xmlns: S3_XMLNS.to_string(),
            location,
            bucket,
            key,
            etag,
        }
    }
}

impl S3XmlResponse for CompleteMultipartUploadResult {
    fn root_element_name() -> &'static str {
        "CompleteMultipartUploadResult"
    }
}

/// ListMultipartUploadsResult - Response for GET /{bucket}?uploads
#[derive(Debug, Clone, Serialize)]
#[serde(rename = "ListMultipartUploadsResult")]
pub struct ListMultipartUploadsResult {
    #[serde(rename = "@xmlns")]
    pub xmlns: String,
    #[serde(rename = "Bucket")]
    pub bucket: String,
    #[serde(rename = "Upload")]
    pub upload: Vec<UploadEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadEntry {
    #[serde(rename = "Key")]
    pub key: String,
    #[serde(rename = "UploadId")]
    pub upload_id: String,
    #[serde(rename = "Initiated", serialize_with = "serialize_timestamp")]
    pub initiated: DateTime<Utc>,
}

impl ListMultipartUploadsResult {
    pub fn new(bucket: String, uploads: Vec<(String, String, DateTime<Utc>)>) -> Self {
        Self {
            xmlns: S3_XMLNS.to_string(),
            bucket,
            upload: uploads
                .into_iter()
                .map(|(key, upload_id, initiated)| UploadEntry {
                    key,
                    upload_id,
                    initiated,
                })
                .collect(),
        }
    }
}

impl S3XmlResponse for ListMultipartUploadsResult {
    fn root_element_name() -> &'static str {
        "ListMultipartUploadsResult"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_list_buckets_xml() {
        let result = ListAllMyBucketsResult::new(
            vec![(
                "test-bucket".to_string(),
                Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            )],
            "test-owner".to_string(),
        );

        let xml = result.to_xml().expect("XML serialization should succeed");
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<ListAllMyBucketsResult"));
        assert!(xml.contains(&format!("xmlns=\"{}\"", S3_XMLNS)));
        assert!(xml.contains("<Name>test-bucket</Name>"));
        assert!(xml.contains("<Owner>"));
        assert!(xml.contains("<ID>test-owner</ID>"));
    }

    #[test]
    fn test_list_objects_xml() {
        let result = ListBucketResult::new(
            "test-bucket".to_string(),
            Some("prefix/".to_string()),
            None,
            1000,
            false,
            vec![(
                "prefix/file.txt".to_string(),
                Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
                "abc123".to_string(),
                1024,
            )],
        );

        let xml = result.to_xml().expect("XML serialization should succeed");
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<ListBucketResult"));
        assert!(xml.contains(&format!("xmlns=\"{}\"", S3_XMLNS)));
        assert!(xml.contains("<Name>test-bucket</Name>"));
        assert!(xml.contains("<Key>prefix/file.txt</Key>"));
        assert!(xml.contains("<Size>1024</Size>"));
        assert!(xml.contains("<StorageClass>STANDARD</StorageClass>"));
    }

    #[test]
    fn test_initiate_multipart_xml() {
        let result = InitiateMultipartUploadResult::new(
            "test-bucket".to_string(),
            "test-key".to_string(),
            "upload-123".to_string(),
        );

        let xml = result.to_xml().expect("XML serialization should succeed");
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<InitiateMultipartUploadResult"));
        assert!(xml.contains(&format!("xmlns=\"{}\"", S3_XMLNS)));
        assert!(xml.contains("<Bucket>test-bucket</Bucket>"));
        assert!(xml.contains("<Key>test-key</Key>"));
        assert!(xml.contains("<UploadId>upload-123</UploadId>"));
    }

    #[test]
    fn test_complete_multipart_xml() {
        let result = CompleteMultipartUploadResult::new(
            "test-bucket".to_string(),
            "test-key".to_string(),
            "etag-123".to_string(),
            "http://localhost:9000".to_string(),
        );

        let xml = result.to_xml().expect("XML serialization should succeed");
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<CompleteMultipartUploadResult"));
        assert!(xml.contains(&format!("xmlns=\"{}\"", S3_XMLNS)));
        assert!(xml.contains("<Bucket>test-bucket</Bucket>"));
        assert!(xml.contains("<ETag>etag-123</ETag>"));
        assert!(xml.contains("<Location>http://localhost:9000/test-bucket/test-key</Location>"));
    }

    #[test]
    fn test_list_uploads_xml() {
        let result = ListMultipartUploadsResult::new(
            "test-bucket".to_string(),
            vec![(
                "test-key".to_string(),
                "upload-123".to_string(),
                Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            )],
        );

        let xml = result.to_xml().expect("XML serialization should succeed");
        assert!(xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<ListMultipartUploadsResult"));
        assert!(xml.contains(&format!("xmlns=\"{}\"", S3_XMLNS)));
        assert!(xml.contains("<Bucket>test-bucket</Bucket>"));
        assert!(xml.contains("<Key>test-key</Key>"));
        assert!(xml.contains("<UploadId>upload-123</UploadId>"));
    }

    #[test]
    fn test_storage_class_display() {
        assert_eq!(StorageClass::Standard.to_string(), "STANDARD");
        assert_eq!(StorageClass::Standard.as_str(), "STANDARD");
    }
}
