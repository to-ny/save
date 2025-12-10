//! Storage, disk, and database metrics.

use prometheus::{
    GaugeVec, IntCounter, IntCounterVec, IntGauge, IntGaugeVec, register_gauge_vec,
    register_int_counter, register_int_counter_vec, register_int_gauge, register_int_gauge_vec,
};
use std::sync::OnceLock;

static GC_FILES_DELETED_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static GC_ERRORS_TOTAL: OnceLock<IntCounter> = OnceLock::new();
static GC_LAST_RUN_SECONDS: OnceLock<IntGauge> = OnceLock::new();
static GC_CYCLES_TOTAL: OnceLock<IntCounterVec> = OnceLock::new();
static DISK_USAGE_BYTES: OnceLock<GaugeVec> = OnceLock::new();
static ROCKSDB_STATS: OnceLock<IntGaugeVec> = OnceLock::new();
static TEMP_FILES_COUNT: OnceLock<IntGauge> = OnceLock::new();
static TEMP_FILES_SIZE_BYTES: OnceLock<IntGauge> = OnceLock::new();

/// Total files deleted by GC.
pub fn gc_files_deleted_total() -> &'static IntCounter {
    GC_FILES_DELETED_TOTAL.get_or_init(|| {
        register_int_counter!(
            "save_gc_files_deleted_total",
            "Total number of files deleted by garbage collection"
        )
        .expect("Failed to register save_gc_files_deleted_total metric")
    })
}

/// Total GC errors.
pub fn gc_errors_total() -> &'static IntCounter {
    GC_ERRORS_TOTAL.get_or_init(|| {
        register_int_counter!(
            "save_gc_errors_total",
            "Total number of garbage collection errors"
        )
        .expect("Failed to register save_gc_errors_total metric")
    })
}

/// Timestamp of last GC run (Unix seconds).
pub fn gc_last_run_seconds() -> &'static IntGauge {
    GC_LAST_RUN_SECONDS.get_or_init(|| {
        register_int_gauge!(
            "save_gc_last_run_seconds",
            "Unix timestamp of the last garbage collection run"
        )
        .expect("Failed to register save_gc_last_run_seconds metric")
    })
}

/// GC cycle counts by result.
pub fn gc_cycles_total() -> &'static IntCounterVec {
    GC_CYCLES_TOTAL.get_or_init(|| {
        register_int_counter_vec!(
            "save_gc_cycles_total",
            "Total number of garbage collection cycles",
            &["result"]
        )
        .expect("Failed to register save_gc_cycles_total metric")
    })
}

/// Disk usage by path and type.
pub fn disk_usage_bytes() -> &'static GaugeVec {
    DISK_USAGE_BYTES.get_or_init(|| {
        register_gauge_vec!(
            "save_disk_usage_bytes",
            "Disk usage in bytes",
            &["path", "type"]
        )
        .expect("Failed to register save_disk_usage_bytes metric")
    })
}

/// RocksDB statistics.
pub fn rocksdb_stats() -> &'static IntGaugeVec {
    ROCKSDB_STATS.get_or_init(|| {
        register_int_gauge_vec!("save_rocksdb_stats", "RocksDB statistics", &["stat"])
            .expect("Failed to register save_rocksdb_stats metric")
    })
}

/// Number of temporary files.
pub fn temp_files_count() -> &'static IntGauge {
    TEMP_FILES_COUNT.get_or_init(|| {
        register_int_gauge!(
            "save_temp_files_count",
            "Number of temporary files in the temp directory"
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

pub(crate) fn init() {
    let _ = gc_files_deleted_total();
    let _ = gc_errors_total();
    let _ = gc_last_run_seconds();
    let _ = gc_cycles_total();
    let _ = disk_usage_bytes();
    let _ = rocksdb_stats();
    let _ = temp_files_count();
    let _ = temp_files_size_bytes();
}
