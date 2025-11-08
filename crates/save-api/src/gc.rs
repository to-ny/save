use crate::metrics::{
    gc_cycles_total, gc_errors_total, gc_files_deleted_total, gc_last_run_seconds,
};
use save_metadata::MetadataStore;
use save_storage::{StorageError, list_temp_files, remove_if_exists};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time;
use tracing::{debug, error, info, warn};

type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Clone)]
pub struct GcConfig {
    pub interval: Duration,
    pub temp_file_max_age: Duration,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(10 * 60),
            temp_file_max_age: Duration::from_secs(60 * 60),
        }
    }
}

pub async fn run_gc_worker(
    metadata: Arc<MetadataStore>,
    temp_dir: PathBuf,
    config: GcConfig,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<()> {
    info!(
        target: "save::gc",
        interval_secs = config.interval.as_secs(),
        max_age_secs = config.temp_file_max_age.as_secs(),
        temp_dir = ?temp_dir,
        "Starting GC worker"
    );

    let mut interval = time::interval(config.interval);
    interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(e) = run_gc_cycle(&metadata, &temp_dir, &config).await {
                    error!(
                        target: "save::gc",
                        error = %e,
                        "GC cycle failed"
                    );
                }
            }
            _ = shutdown.recv() => {
                info!(target: "save::gc", "GC worker shutting down gracefully");
                return Ok(());
            }
        }
    }
}

async fn run_gc_cycle(metadata: &MetadataStore, temp_dir: &Path, config: &GcConfig) -> Result<()> {
    let start = SystemTime::now();
    debug!(target: "save::gc", "Starting GC cycle");

    let active_upload_ids = match get_active_upload_ids(metadata).await {
        Ok(ids) => ids,
        Err(e) => {
            gc_cycles_total().with_label_values(&["error"]).inc();
            return Err(e);
        }
    };

    debug!(
        target: "save::gc",
        active_uploads = active_upload_ids.len(),
        "Found active multipart uploads"
    );

    let files = match list_temp_files(temp_dir).await {
        Ok(f) => f,
        Err(e) => {
            gc_cycles_total().with_label_values(&["error"]).inc();
            return Err(e);
        }
    };

    debug!(
        target: "save::gc",
        total_files = files.len(),
        "Scanned temp directory"
    );

    let mut deleted_count = 0;
    let mut error_count = 0;
    let now = SystemTime::now();

    for (path, modified) in files {
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);

        if age < config.temp_file_max_age {
            continue;
        }

        if should_keep_file(&path, &active_upload_ids) {
            debug!(
                target: "save::gc",
                path = ?path,
                "Keeping file (referenced by active upload)"
            );
            continue;
        }

        match remove_if_exists(&path).await {
            Ok(true) => {
                deleted_count += 1;
                gc_files_deleted_total().inc();
                info!(
                    target: "save::gc",
                    path = ?path,
                    age_secs = age.as_secs(),
                    "Deleted orphaned temp file"
                );
            }
            Ok(false) => {
                debug!(target: "save::gc", path = ?path, "File already deleted");
            }
            Err(e) => {
                error_count += 1;
                gc_errors_total().inc();
                warn!(
                    target: "save::gc",
                    path = ?path,
                    error = %e,
                    "Failed to delete temp file"
                );
            }
        }
    }

    // Update last run timestamp
    if let Ok(duration) = start.duration_since(UNIX_EPOCH) {
        gc_last_run_seconds().set(duration.as_secs() as i64);
    }

    gc_cycles_total().with_label_values(&["success"]).inc();

    let elapsed = start.elapsed().unwrap_or(Duration::ZERO);
    info!(
        target: "save::gc",
        deleted = deleted_count,
        errors = error_count,
        duration_ms = elapsed.as_millis(),
        "GC cycle completed"
    );

    Ok(())
}

async fn get_active_upload_ids(metadata: &MetadataStore) -> Result<HashSet<String>> {
    let uploads = metadata
        .list_all_multipart_uploads()
        .await
        .map_err(|e| StorageError::Io(std::io::Error::other(e)))?;

    Ok(uploads.into_iter().map(|u| u.upload_id).collect())
}

fn should_keep_file(path: &Path, active_upload_ids: &HashSet<String>) -> bool {
    if let Some(parent) = path.parent()
        && let Some(upload_id) = parent.file_name().and_then(|n| n.to_str())
        && active_upload_ids.contains(upload_id)
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use save_metadata::MetadataStore;
    use std::time::Duration;

    #[test]
    fn test_should_keep_file_with_active_upload() {
        let mut active_uploads = HashSet::new();
        active_uploads.insert("upload123".to_string());

        let path = PathBuf::from("/tmp/temp/parts/upload123/1");
        assert!(should_keep_file(&path, &active_uploads));
    }

    #[test]
    fn test_should_keep_file_without_active_upload() {
        let active_uploads = HashSet::new();

        let path = PathBuf::from("/tmp/temp/parts/upload123/1");
        assert!(!should_keep_file(&path, &active_uploads));
    }

    #[test]
    fn test_should_keep_file_temp_file() {
        let active_uploads = HashSet::new();

        let path = PathBuf::from("/tmp/temp/abc123.tmp");
        assert!(!should_keep_file(&path, &active_uploads));
    }

    #[test]
    fn test_gc_config_default() {
        let config = GcConfig::default();
        assert_eq!(config.interval, Duration::from_secs(10 * 60));
        assert_eq!(config.temp_file_max_age, Duration::from_secs(60 * 60));
    }

    #[tokio::test]
    async fn test_gc_cycle_deletes_old_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        let old_file = temp_dir.path().join("old.tmp");
        tokio::fs::write(&old_file, b"old data").await.unwrap();

        let metadata_old = tokio::fs::metadata(&old_file).await.unwrap();
        let old_time = metadata_old.modified().unwrap();
        let two_hours_ago = old_time - Duration::from_secs(2 * 60 * 60);

        #[cfg(unix)]
        {
            filetime::set_file_mtime(
                &old_file,
                filetime::FileTime::from_system_time(two_hours_ago),
            )
            .ok();
        }

        let config = GcConfig {
            interval: Duration::from_secs(60),
            temp_file_max_age: Duration::from_secs(60 * 60),
        };

        let result = run_gc_cycle(&metadata, temp_dir.path(), &config).await;
        assert!(result.is_ok(), "GC cycle should succeed");
    }

    #[tokio::test]
    async fn test_gc_cycle_preserves_recent_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        let recent_file = temp_dir.path().join("recent.tmp");
        tokio::fs::write(&recent_file, b"recent data")
            .await
            .unwrap();

        let config = GcConfig {
            interval: Duration::from_secs(60),
            temp_file_max_age: Duration::from_secs(60 * 60),
        };

        let result = run_gc_cycle(&metadata, temp_dir.path(), &config).await;
        assert!(result.is_ok(), "GC cycle should succeed");
        assert!(recent_file.exists(), "Recent file should not be deleted");
    }

    #[tokio::test]
    async fn test_gc_cycle_protects_active_multipart_uploads() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        metadata.create_bucket("test-bucket").await.unwrap();

        metadata
            .initiate_multipart_upload("test-bucket", "test-key", "upload123", None)
            .await
            .unwrap();

        let parts_dir = temp_dir.path().join("parts").join("upload123");
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let part_file = parts_dir.join("1");
        tokio::fs::write(&part_file, b"part data").await.unwrap();

        let config = GcConfig {
            interval: Duration::from_secs(60),
            temp_file_max_age: Duration::from_secs(0),
        };

        let result = run_gc_cycle(&metadata, temp_dir.path(), &config).await;
        assert!(result.is_ok(), "GC cycle should succeed");
        assert!(
            part_file.exists(),
            "Active upload part file should be protected"
        );
    }

    #[tokio::test]
    async fn test_gc_cycle_deletes_orphaned_multipart_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        let parts_dir = temp_dir.path().join("parts").join("orphaned-upload");
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let part_file = parts_dir.join("1");
        tokio::fs::write(&part_file, b"orphaned part")
            .await
            .unwrap();

        let config = GcConfig {
            interval: Duration::from_secs(60),
            temp_file_max_age: Duration::from_secs(0),
        };

        let result = run_gc_cycle(&metadata, temp_dir.path(), &config).await;
        assert!(result.is_ok(), "GC cycle should succeed");
    }

    #[tokio::test]
    async fn test_gc_cycle_handles_empty_temp_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        let config = GcConfig::default();

        let result = run_gc_cycle(&metadata, temp_dir.path(), &config).await;
        assert!(result.is_ok(), "GC cycle should handle empty directory");
    }

    #[tokio::test]
    async fn test_get_active_upload_ids() {
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = MetadataStore::new(metadata_dir.path()).unwrap();

        metadata.create_bucket("bucket1").await.unwrap();
        metadata.create_bucket("bucket2").await.unwrap();

        metadata
            .initiate_multipart_upload("bucket1", "key1", "upload1", None)
            .await
            .unwrap();

        metadata
            .initiate_multipart_upload("bucket1", "key2", "upload2", None)
            .await
            .unwrap();

        metadata
            .initiate_multipart_upload("bucket2", "key3", "upload3", None)
            .await
            .unwrap();

        let upload_ids = get_active_upload_ids(&metadata).await.unwrap();

        assert_eq!(upload_ids.len(), 3);
        assert!(upload_ids.contains("upload1"));
        assert!(upload_ids.contains("upload2"));
        assert!(upload_ids.contains("upload3"));
    }

    #[tokio::test]
    async fn test_gc_concurrent_with_active_operations() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        metadata.create_bucket("test-bucket").await.unwrap();
        metadata
            .initiate_multipart_upload("test-bucket", "key", "upload1", None)
            .await
            .unwrap();

        let parts_dir = temp_dir.path().join("parts").join("upload1");
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let part_file = parts_dir.join("1");
        tokio::fs::write(&part_file, b"active part").await.unwrap();

        let old_file = temp_dir.path().join("old.tmp");
        tokio::fs::write(&old_file, b"old data").await.unwrap();

        #[cfg(unix)]
        {
            use std::time::SystemTime;
            let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 60 * 60);
            filetime::set_file_mtime(
                &old_file,
                filetime::FileTime::from_system_time(two_hours_ago),
            )
            .ok();
        }

        let config = GcConfig {
            interval: Duration::from_millis(100),
            temp_file_max_age: Duration::from_secs(60 * 60),
        };

        let metadata_clone = Arc::clone(&metadata);
        let temp_dir_path = temp_dir.path().to_path_buf();
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        let gc_handle = tokio::spawn(async move {
            run_gc_worker(metadata_clone, temp_dir_path, config, shutdown_rx)
                .await
                .unwrap();
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(part_file.exists(), "Active upload part should be protected");

        metadata
            .abort_multipart_upload("test-bucket", "key", "upload1")
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(200)).await;

        let _ = shutdown_tx.send(());
        let _ = gc_handle.await;
    }

    #[tokio::test]
    async fn test_gc_with_concurrent_file_creation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let metadata_dir = tempfile::tempdir().unwrap();
        let metadata = Arc::new(MetadataStore::new(metadata_dir.path()).unwrap());

        let config = GcConfig {
            interval: Duration::from_millis(50),
            temp_file_max_age: Duration::from_secs(0),
        };

        let temp_dir_path = temp_dir.path().to_path_buf();
        let temp_dir_clone = temp_dir_path.clone();
        let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

        let gc_handle = tokio::spawn(async move {
            run_gc_worker(Arc::clone(&metadata), temp_dir_path, config, shutdown_rx)
                .await
                .unwrap();
        });

        let writer_handle = tokio::spawn(async move {
            for i in 0..10 {
                let file_path = temp_dir_clone.join(format!("file{}.tmp", i));
                tokio::fs::write(&file_path, b"data").await.unwrap();
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });

        let _ = writer_handle.await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        let _ = shutdown_tx.send(());
        let _ = gc_handle.await;
    }
}
