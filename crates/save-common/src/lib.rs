pub mod cluster;
pub mod config;
pub mod error;
pub mod grpc;
pub mod ports;
pub mod retry;
pub mod s3_error;
pub mod s3_responses;
pub mod tls;
pub mod tracing;
pub mod types;
pub mod validation;

pub use config::{RetrySettings, TlsConfig};
pub use error::{Error, Result};
pub use grpc::{
    BoxBody, create_grpc_error_response, create_grpc_response, create_grpc_streaming_response,
    parse_grpc_frame,
};
pub use retry::{RetryConfig, retry_with_backoff};

impl From<&RetrySettings> for RetryConfig {
    fn from(settings: &RetrySettings) -> Self {
        Self {
            max_retries: settings.max_attempts,
            initial_delay: std::time::Duration::from_millis(settings.initial_delay_ms),
            max_delay: std::time::Duration::from_millis(settings.max_delay_ms),
            multiplier: 2.0,
            jitter: settings.jitter,
        }
    }
}
pub use s3_error::{S3Error, S3ErrorCode};
pub use s3_responses::{
    CompleteMultipartUploadResult, InitiateMultipartUploadResult, ListAllMyBucketsResult,
    ListBucketResult, ListBucketResultV2, ListMultipartUploadsResult, S3_XMLNS, S3XmlResponse,
    SerializationError, StorageClass,
};
pub use tls::{load_client_tls_config, load_server_tls_config};
pub use types::Bucket;
pub use validation::{validate_bucket_name, validate_object_key};
