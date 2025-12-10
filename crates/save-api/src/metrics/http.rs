//! HTTP and API request metrics.

use prometheus::{
    HistogramVec, IntCounter, IntCounterVec, IntGauge, register_histogram_vec,
    register_int_counter, register_int_counter_vec, register_int_gauge,
};
use std::sync::OnceLock;

static HTTP_REQUESTS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static HTTP_REQUEST_DURATION_SECONDS: OnceLock<HistogramVec> = OnceLock::new();
static OBJECT_SIZE_BYTES: OnceLock<HistogramVec> = OnceLock::new();
static RESPONSE_SIZE_BYTES: OnceLock<HistogramVec> = OnceLock::new();
static MULTIPART_UPLOADS_IN_PROGRESS: OnceLock<IntGauge> = OnceLock::new();
static ATOMIC_PUT_OPERATIONS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static MULTIPART_CLEANUP_FAILURES_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static AUTH_EVENTS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static IN_FLIGHT_REQUESTS: OnceLock<IntGauge> = OnceLock::new();
static ERRORS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();

const KB: f64 = 1024.0;
const MB: f64 = 1024.0 * KB;
const GB: f64 = 1024.0 * MB;

/// HTTP request count by endpoint, method, and status code.
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

/// Object size distribution by operation type.
pub fn object_size_bytes() -> &'static HistogramVec {
    OBJECT_SIZE_BYTES.get_or_init(|| {
        register_histogram_vec!(
            "save_object_size_bytes",
            "Size of objects in bytes",
            &["operation"],
            vec![
                KB,
                10.0 * KB,
                100.0 * KB,
                MB,
                10.0 * MB,
                100.0 * MB,
                GB,
                10.0 * GB,
            ]
        )
        .expect("Failed to register save_object_size_bytes metric")
    })
}

/// Response size distribution by endpoint.
pub fn response_size_bytes() -> &'static HistogramVec {
    RESPONSE_SIZE_BYTES.get_or_init(|| {
        register_histogram_vec!(
            "save_response_size_bytes",
            "Size of HTTP responses in bytes",
            &["endpoint"],
            vec![KB, 10.0 * KB, 100.0 * KB, MB, 10.0 * MB, 100.0 * MB, GB,]
        )
        .expect("Failed to register save_response_size_bytes metric")
    })
}

/// Number of multipart uploads currently in progress.
pub fn multipart_uploads_in_progress() -> &'static IntGauge {
    MULTIPART_UPLOADS_IN_PROGRESS.get_or_init(|| {
        register_int_gauge!(
            "save_multipart_uploads_in_progress",
            "Number of multipart uploads currently in progress"
        )
        .expect("Failed to register save_multipart_uploads_in_progress metric")
    })
}

/// Total atomic PUT operations by stage and result.
pub fn atomic_put_operations_total() -> &'static IntCounterVec {
    ATOMIC_PUT_OPERATIONS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_atomic_put_operations_total",
            "Total number of atomic PUT operations",
            &["stage", "result"]
        )
        .expect("Failed to register save_atomic_put_operations_total metric")
    })
}

/// Total multipart cleanup failures.
pub fn multipart_cleanup_failures_total() -> &'static IntCounter {
    MULTIPART_CLEANUP_FAILURES_TOTAL.get_or_init(|| {
        register_int_counter!(
            "save_multipart_cleanup_failures_total",
            "Total number of multipart cleanup failures"
        )
        .expect("Failed to register save_multipart_cleanup_failures_total metric")
    })
}

/// Authentication events by result and reason.
pub fn auth_events_total() -> &'static IntCounterVec {
    AUTH_EVENTS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_auth_events_total",
            "Total number of authentication events",
            &["result", "reason"]
        )
        .expect("Failed to register save_auth_events_total metric")
    })
}

/// Number of requests currently in flight.
pub fn in_flight_requests() -> &'static IntGauge {
    IN_FLIGHT_REQUESTS.get_or_init(|| {
        register_int_gauge!(
            "save_in_flight_requests",
            "Number of requests currently being processed"
        )
        .expect("Failed to register save_in_flight_requests metric")
    })
}

/// Error counts by type and endpoint.
pub fn errors_total() -> &'static IntCounterVec {
    ERRORS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_errors_total",
            "Total number of errors",
            &["error_type", "endpoint"]
        )
        .expect("Failed to register save_errors_total metric")
    })
}

pub(crate) fn init() {
    let _ = http_requests_total();
    let _ = http_request_duration_seconds();
    let _ = object_size_bytes();
    let _ = response_size_bytes();
    let _ = multipart_uploads_in_progress();
    let _ = atomic_put_operations_total();
    let _ = multipart_cleanup_failures_total();
    let _ = auth_events_total();
    let _ = in_flight_requests();
    let _ = errors_total();
}
