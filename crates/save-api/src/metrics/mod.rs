//! Metrics collection and export.
//!
//! Organized by domain:
//! - `http`: HTTP/API request metrics
//! - `storage`: Disk, GC, and database metrics
//! - `cluster`: Raft, replication, and cluster health metrics
//! - `grpc`: gRPC request and stream metrics

mod cluster;
mod grpc;
mod http;
mod storage;

use prometheus::{Encoder, TextEncoder};
use thiserror::Error;

// Re-export all metrics
pub use cluster::*;
pub use grpc::*;
pub use http::*;
pub use storage::*;

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("Failed to encode metrics: {0}")]
    Encode(#[from] prometheus::Error),
    #[error("Failed to convert metrics to UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Initialize all metrics (pre-register with Prometheus).
pub fn init_metrics() {
    http::init();
    storage::init();
    cluster::init();
    grpc::init();
}

/// Encode all metrics in Prometheus text format.
pub fn encode_metrics() -> Result<String, MetricsError> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();

    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_encode() {
        init_metrics();

        http_requests_total()
            .with_label_values(&["/health", "GET", "200"])
            .inc();

        http_request_duration_seconds()
            .with_label_values(&["/health", "GET"])
            .observe(0.1);

        object_size_bytes()
            .with_label_values(&["put"])
            .observe(1024.0);

        let result = encode_metrics();
        assert!(result.is_ok());

        let metrics = result.unwrap();
        assert!(metrics.contains("save_http_requests_total"));
        assert!(metrics.contains("save_http_request_duration_seconds"));
        assert!(metrics.contains("save_object_size_bytes"));
    }

    #[test]
    fn test_record_request() {
        http_requests_total()
            .with_label_values(&["/bucket/object", "PUT", "200"])
            .inc();

        http_request_duration_seconds()
            .with_label_values(&["/bucket/object", "PUT"])
            .observe(0.5);

        let metrics = encode_metrics().unwrap();
        assert!(metrics.contains("save_http_requests_total"));
    }

    #[test]
    fn test_multipart_gauge() {
        let initial = multipart_uploads_in_progress().get();

        multipart_uploads_in_progress().inc();
        assert_eq!(multipart_uploads_in_progress().get(), initial + 1);

        multipart_uploads_in_progress().dec();
        assert_eq!(multipart_uploads_in_progress().get(), initial);
    }
}
