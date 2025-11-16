use prometheus::{
    Encoder, GaugeVec, HistogramVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, TextEncoder,
    register_gauge_vec, register_histogram_vec, register_int_counter, register_int_counter_vec,
    register_int_gauge, register_int_gauge_vec,
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

// Enhanced system metrics
static DISK_USAGE_BYTES: OnceLock<GaugeVec> = OnceLock::new();
static IN_FLIGHT_REQUESTS: OnceLock<IntGauge> = OnceLock::new();

// RocksDB metrics
static ROCKSDB_STATS: OnceLock<IntGaugeVec> = OnceLock::new();

// Error metrics
static ERRORS_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();

// Temp file metrics
static TEMP_FILES_COUNT: OnceLock<IntGauge> = OnceLock::new();
static TEMP_FILES_SIZE_BYTES: OnceLock<IntGauge> = OnceLock::new();

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

/// Disk usage in bytes by type (total, used, available).
///
/// Labels have bounded cardinality:
/// - `type`: Fixed set of values ("total", "used", "available")
/// - `path`: Fixed set of paths (data_path, metadata_path)
pub fn disk_usage_bytes() -> &'static GaugeVec {
    DISK_USAGE_BYTES.get_or_init(|| {
        register_gauge_vec!(
            "save_disk_usage_bytes",
            "Disk usage in bytes by type and path",
            &["path", "type"]
        )
        .expect("Failed to register save_disk_usage_bytes metric")
    })
}

/// Number of in-flight HTTP requests.
pub fn in_flight_requests() -> &'static IntGauge {
    IN_FLIGHT_REQUESTS.get_or_init(|| {
        register_int_gauge!(
            "save_in_flight_requests",
            "Number of HTTP requests currently being processed"
        )
        .expect("Failed to register save_in_flight_requests metric")
    })
}

/// RocksDB statistics by metric name.
///
/// Labels have bounded cardinality:
/// - `stat`: Fixed set of values ("block_cache_hits", "block_cache_misses", "memtable_size_bytes", "estimate_num_keys")
pub fn rocksdb_stats() -> &'static IntGaugeVec {
    ROCKSDB_STATS.get_or_init(|| {
        register_int_gauge_vec!(
            "save_rocksdb_stats",
            "RocksDB statistics by metric name",
            &["stat"]
        )
        .expect("Failed to register save_rocksdb_stats metric")
    })
}

/// Total errors by type and endpoint.
///
/// Labels have bounded cardinality:
/// - `error_type`: Bounded set of error categories ("storage", "metadata", "auth", "validation", "internal")
/// - `endpoint`: Normalized to ~5 values (same as http_requests_total)
pub fn errors_total() -> &'static IntCounterVec {
    ERRORS_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_errors_total",
            "Total errors by type and endpoint",
            &["error_type", "endpoint"]
        )
        .expect("Failed to register save_errors_total metric")
    })
}

/// Number of temporary files currently on disk.
pub fn temp_files_count() -> &'static IntGauge {
    TEMP_FILES_COUNT.get_or_init(|| {
        register_int_gauge!(
            "save_temp_files_count",
            "Number of temporary files currently on disk"
        )
        .expect("Failed to register save_temp_files_count metric")
    })
}

/// Total size of temporary files in bytes.
pub fn temp_files_size_bytes() -> &'static IntGauge {
    TEMP_FILES_SIZE_BYTES.get_or_init(|| {
        register_int_gauge!(
            "save_temp_files_size_bytes",
            "Total size of temporary files in bytes"
        )
        .expect("Failed to register save_temp_files_size_bytes metric")
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
    let _ = disk_usage_bytes();
    let _ = in_flight_requests();
    let _ = rocksdb_stats();
    let _ = errors_total();
    let _ = temp_files_count();
    let _ = temp_files_size_bytes();
}

pub fn encode_metrics() -> Result<String, MetricsError> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();

    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}

#[cfg(target_family = "unix")]
pub fn collect_disk_usage(path: &std::path::Path, label: &str) {
    use nix::sys::statvfs::statvfs;

    if let Ok(stat) = statvfs(path) {
        let total_bytes = stat.blocks() * stat.block_size();
        let available_bytes = stat.blocks_available() * stat.block_size();
        let used_bytes = total_bytes - available_bytes;

        disk_usage_bytes()
            .with_label_values(&[label, "total"])
            .set(total_bytes as f64);
        disk_usage_bytes()
            .with_label_values(&[label, "used"])
            .set(used_bytes as f64);
        disk_usage_bytes()
            .with_label_values(&[label, "available"])
            .set(available_bytes as f64);
    }
}

#[cfg(not(target_family = "unix"))]
pub fn collect_disk_usage(_path: &std::path::Path, _label: &str) {}

pub fn collect_database_stats(stats: &save_metadata::DatabaseStats) {
    if let Some(hits) = stats.block_cache_hits {
        rocksdb_stats()
            .with_label_values(&["block_cache_hits"])
            .set(hits as i64);
    }
    if let Some(misses) = stats.block_cache_misses {
        rocksdb_stats()
            .with_label_values(&["block_cache_misses"])
            .set(misses as i64);
    }

    if let Some(mem) = stats.table_readers_mem_bytes {
        rocksdb_stats()
            .with_label_values(&["table_readers_mem_bytes"])
            .set(mem as i64);
    }
    if let Some(mem) = stats.memtable_size_bytes {
        rocksdb_stats()
            .with_label_values(&["memtable_size_bytes"])
            .set(mem as i64);
    }

    if let Some(keys) = stats.estimate_num_keys {
        rocksdb_stats()
            .with_label_values(&["estimate_num_keys"])
            .set(keys as i64);
    }
}

pub fn collect_temp_file_stats(temp_dir: &std::path::Path) {
    let mut count = 0;
    let mut total_size = 0u64;

    let mut dirs_to_process = vec![temp_dir.to_path_buf()];

    while let Some(dir) = dirs_to_process.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        count += 1;
                        total_size += metadata.len();
                    } else if metadata.is_dir() {
                        dirs_to_process.push(entry.path());
                    }
                }
            }
        }
    }

    temp_files_count().set(count);
    temp_files_size_bytes().set(total_size as i64);
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
        let initial = multipart_uploads_in_progress().get();

        multipart_uploads_in_progress().inc();
        assert_eq!(multipart_uploads_in_progress().get(), initial + 1);

        multipart_uploads_in_progress().dec();
        assert_eq!(multipart_uploads_in_progress().get(), initial);
    }
}
