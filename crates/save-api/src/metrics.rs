use prometheus::{
    register_histogram_vec, register_int_counter_vec, register_int_gauge, Encoder, HistogramVec,
    IntCounterVec, IntGauge, TextEncoder,
};
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MetricsError {
    #[error("Failed to encode metrics: {0}")]
    Encode(#[from] prometheus::Error),
    #[error("Failed to convert metrics to UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

const KB: f64 = 1024.0;
const MB: f64 = 1024.0 * KB;
const GB: f64 = 1024.0 * MB;

static HTTP_REQUESTS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static HTTP_REQUEST_DURATION_SECONDS: OnceLock<HistogramVec> = OnceLock::new();
static OBJECT_SIZE_BYTES: OnceLock<HistogramVec> = OnceLock::new();
static MULTIPART_UPLOADS_IN_PROGRESS: OnceLock<IntGauge> = OnceLock::new();

/// HTTP request count by endpoint, method, and status code.
///
/// Labels have bounded cardinality:
/// - `endpoint`: Normalized to ~5 values (/health, /metrics, /, /{bucket}, /{bucket}/{key})
/// - `method`: Limited to standard HTTP methods (~9 values: GET, PUT, POST, DELETE, HEAD, etc.)
/// - `status`: Limited to HTTP status codes (~60 common values: 200, 404, 500, etc.)
pub fn http_requests_total() -> &'static IntCounterVec {
    HTTP_REQUESTS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_http_requests_total",
            "Total number of HTTP requests",
            &["endpoint", "method", "status"]
        )
        .expect("Failed to register save_http_requests_total metric")
    })
}

/// HTTP request latency by endpoint and method.
///
/// Labels have bounded cardinality:
/// - `endpoint`: Normalized to ~5 values (see http_requests_total)
/// - `method`: Limited to standard HTTP methods (~9 values)
pub fn http_request_duration_seconds() -> &'static HistogramVec {
    HTTP_REQUEST_DURATION_SECONDS.get_or_init(|| {
        register_histogram_vec!(
            "save_http_request_duration_seconds",
            "HTTP request latency in seconds",
            &["endpoint", "method"],
            vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
        )
        .expect("Failed to register save_http_request_duration_seconds metric")
    })
}

/// Object size distribution for PUT operations.
///
/// Labels have bounded cardinality:
/// - `operation`: Fixed set of values ("put", "multipart_complete")
pub fn object_size_bytes() -> &'static HistogramVec {
    OBJECT_SIZE_BYTES.get_or_init(|| {
        register_histogram_vec!(
            "save_object_size_bytes",
            "Object size in bytes for PUT operations",
            &["operation"],
            vec![KB, 10.0 * KB, 100.0 * KB, MB, 10.0 * MB, 100.0 * MB, GB, 5.0 * GB]
        )
        .expect("Failed to register save_object_size_bytes metric")
    })
}

/// Number of active multipart uploads.
///
/// This gauge tracks uploads from initiation to completion/abortion.
pub fn multipart_uploads_in_progress() -> &'static IntGauge {
    MULTIPART_UPLOADS_IN_PROGRESS.get_or_init(|| {
        register_int_gauge!(
            "save_multipart_uploads_in_progress",
            "Number of active multipart uploads"
        )
        .expect("Failed to register save_multipart_uploads_in_progress metric")
    })
}

pub fn init_metrics() {
    let _ = http_requests_total();
    let _ = http_request_duration_seconds();
    let _ = object_size_bytes();
    let _ = multipart_uploads_in_progress();
}

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
        assert!(metrics.contains("save_multipart_uploads_in_progress"));
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
    fn test_record_object_size() {
        object_size_bytes()
            .with_label_values(&["put"])
            .observe(1048576.0); // 1 MB

        let metrics = encode_metrics().unwrap();
        assert!(metrics.contains("save_object_size_bytes"));
    }

    #[test]
    fn test_multipart_gauge() {
        multipart_uploads_in_progress().inc();
        assert_eq!(multipart_uploads_in_progress().get(), 1);

        multipart_uploads_in_progress().dec();
        assert_eq!(multipart_uploads_in_progress().get(), 0);
    }
}
