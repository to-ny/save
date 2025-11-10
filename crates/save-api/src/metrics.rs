use prometheus::{
    Encoder, HistogramVec, IntCounter, IntCounterVec, IntGauge, TextEncoder,
    register_histogram_vec, register_int_counter, register_int_counter_vec, register_int_gauge,
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
static RESPONSE_SIZE_BYTES: OnceLock<HistogramVec> = OnceLock::new();
static MULTIPART_UPLOADS_IN_PROGRESS: OnceLock<IntGauge> = OnceLock::new();
static ATOMIC_PUT_OPERATIONS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static GC_FILES_DELETED_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static GC_ERRORS_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static GC_LAST_RUN_SECONDS: OnceLock<IntGauge> = OnceLock::new();
static GC_CYCLES_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static MULTIPART_CLEANUP_FAILURES_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static AUTH_EVENTS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();

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
            vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
            ]
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
            vec![
                KB,
                10.0 * KB,
                100.0 * KB,
                MB,
                10.0 * MB,
                100.0 * MB,
                GB,
                5.0 * GB
            ]
        )
        .expect("Failed to register save_object_size_bytes metric")
    })
}

/// Response size distribution for list/query operations.
///
/// Labels have bounded cardinality:
/// - `endpoint`: Fixed set of values ("list_buckets", "list_objects", "list_multipart", "initiate_multipart", "complete_multipart")
pub fn response_size_bytes() -> &'static HistogramVec {
    RESPONSE_SIZE_BYTES.get_or_init(|| {
        register_histogram_vec!(
            "save_response_size_bytes",
            "Response payload size in bytes for list/query operations",
            &["endpoint"],
            vec![KB, 10.0 * KB, 100.0 * KB, MB, 10.0 * MB,]
        )
        .expect("Failed to register save_response_size_bytes metric")
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

/// Total atomic PUT operations by stage and result.
pub fn atomic_put_operations_total() -> &'static IntCounterVec {
    ATOMIC_PUT_OPERATIONS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_atomic_put_operations_total",
            "Total atomic PUT operations by stage and result",
            &["stage", "result"]
        )
        .expect("Failed to register save_atomic_put_operations_total metric")
    })
}

pub(crate) fn gc_files_deleted_total() -> &'static IntCounter {
    GC_FILES_DELETED_TOTAL.get_or_init(|| {
        register_int_counter!(
            "save_gc_files_deleted_total",
            "Total number of files deleted by GC"
        )
        .expect("Failed to register save_gc_files_deleted_total metric")
    })
}

pub(crate) fn gc_errors_total() -> &'static IntCounter {
    GC_ERRORS_TOTAL.get_or_init(|| {
        register_int_counter!("save_gc_errors_total", "Total number of GC errors")
            .expect("Failed to register save_gc_errors_total metric")
    })
}

pub(crate) fn gc_last_run_seconds() -> &'static IntGauge {
    GC_LAST_RUN_SECONDS.get_or_init(|| {
        register_int_gauge!(
            "save_gc_last_run_seconds",
            "Unix timestamp of last successful GC run"
        )
        .expect("Failed to register save_gc_last_run_seconds metric")
    })
}

pub(crate) fn gc_cycles_total() -> &'static IntCounterVec {
    GC_CYCLES_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_gc_cycles_total",
            "Total number of GC cycles by result",
            &["result"]
        )
        .expect("Failed to register save_gc_cycles_total metric")
    })
}

pub(crate) fn multipart_cleanup_failures_total() -> &'static IntCounter {
    MULTIPART_CLEANUP_FAILURES_TOTAL.get_or_init(|| {
        register_int_counter!(
            "save_multipart_cleanup_failures_total",
            "Total number of multipart upload cleanup failures"
        )
        .expect("Failed to register save_multipart_cleanup_failures_total metric")
    })
}

/// Authentication events by result and reason.
///
/// Labels have bounded cardinality:
/// - `result`: Fixed set of values ("success", "failure")
/// - `reason`: Fixed set of failure reasons ("missing_auth", "invalid_format", "key_mismatch", "signature_mismatch", "time_skewed", "missing_date")
pub fn auth_events_total() -> &'static IntCounterVec {
    AUTH_EVENTS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_auth_events_total",
            "Total authentication events by result and reason",
            &["result", "reason"]
        )
        .expect("Failed to register save_auth_events_total metric")
    })
}

pub fn init_metrics() {
    let _ = http_requests_total();
    let _ = http_request_duration_seconds();
    let _ = object_size_bytes();
    let _ = multipart_uploads_in_progress();
    let _ = atomic_put_operations_total();
    let _ = gc_files_deleted_total();
    let _ = gc_errors_total();
    let _ = gc_last_run_seconds();
    let _ = gc_cycles_total();
    let _ = multipart_cleanup_failures_total();
    let _ = auth_events_total();
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
