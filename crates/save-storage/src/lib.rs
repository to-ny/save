mod backend;
pub mod cluster;
mod error;
mod factory;
mod layout;
mod local_backend;
mod replicated_backend;
pub mod replication;

#[cfg(test)]
mod tests;

pub use backend::{HealthStatus, StorageBackend, TempHandle};
pub use error::{Result, StorageError};
pub use factory::{StorageSetup, create_storage_backend};
pub use local_backend::LocalBackend;
pub use replicated_backend::ReplicatedBackend;

use async_trait::async_trait;
use layout::StorageLayout;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncWriteExt};
use tracing::{debug, instrument, warn};

/// Storage operations required by the replication service.
///
/// This trait abstracts the storage layer so that `ReplicationService`
/// doesn't depend on the concrete `ObjectStorage` implementation.
#[async_trait]
pub trait ReplicationStorage: Send + Sync + 'static {
    /// Write object data directly (non-2PC path).
    async fn put_object(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Create a temp object handle for streaming writes.
    async fn create_temp_object(&self, key: &str) -> Result<TempObject>;

    /// Write object to temp file in one shot (2PC prepare phase).
    async fn write_temp_object(&self, key: &str, data: &[u8]) -> Result<TempObject>;

    /// Commit a temp object to its final location (2PC commit phase).
    async fn commit_object(&self, temp_object: TempObject) -> Result<()>;

    /// Get object file for reading.
    async fn get_object(&self, key: &str) -> Result<fs::File>;

    /// Get object metadata (size and checksum).
    async fn object_info(&self, key: &str) -> Result<(u64, String)>;

    /// Delete an object.
    async fn delete_object(&self, key: &str) -> Result<()>;
}

/// Opaque handle to a temporary object with automatic cleanup on drop.
#[derive(Debug)]
pub struct TempObject {
    temp_path: PathBuf,
    final_path: PathBuf,
    committed: bool,
}

impl TempObject {
    fn new(temp_path: PathBuf, final_path: PathBuf) -> Self {
        Self {
            temp_path,
            final_path,
            committed: false,
        }
    }

    fn mark_committed(&mut self) {
        self.committed = true;
    }

    #[doc(hidden)]
    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }
}

impl Drop for TempObject {
    fn drop(&mut self) {
        if !self.committed && self.temp_path.exists() {
            debug!(path = ?self.temp_path, "Auto-cleaning up uncommitted temp object");

            if let Err(e) = std::fs::remove_file(&self.temp_path) {
                warn!(
                    error = %e,
                    path = ?self.temp_path,
                    "Failed to cleanup temp object in Drop - possible resource leak"
                );
            }
        }
    }
}

/// Synchronizes file to disk.
#[instrument(skip(path), fields(path = ?path.as_ref()))]
pub async fn fsync_file<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref().to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .map_err(|e| {
                StorageError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to open file for fsync at {:?}: {}", path, e),
                ))
            })?;

        file.sync_all().map_err(|e| {
            StorageError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to fsync file at {:?}: {}", path, e),
            ))
        })?;

        Ok(())
    })
    .await
    .map_err(|e| StorageError::Io(std::io::Error::other(e)))?
}

/// Synchronizes directory metadata to disk.
#[instrument(skip(path), fields(path = ?path.as_ref()))]
pub async fn fsync_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref().to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).map_err(|e| {
            StorageError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to open directory for fsync at {:?}: {}", path, e),
            ))
        })?;

        file.sync_all().map_err(|e| {
            StorageError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to fsync directory at {:?}: {}", path, e),
            ))
        })?;

        Ok(())
    })
    .await
    .map_err(|e| StorageError::Io(std::io::Error::other(e)))?
}

#[derive(Debug)]
pub struct ObjectStorage {
    layout: StorageLayout,
    fsync_mode: String,
}

impl ObjectStorage {
    pub async fn new<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        Self::new_with_fsync_mode(base_path, "data").await
    }

    pub async fn new_with_fsync_mode<P: AsRef<Path>>(
        base_path: P,
        fsync_mode: &str,
    ) -> Result<Self> {
        let layout = StorageLayout::new(base_path);

        fs::create_dir_all(layout.objects_dir()).await?;
        fs::create_dir_all(layout.temp_dir()).await?;

        Ok(Self {
            layout,
            fsync_mode: fsync_mode.to_string(),
        })
    }

    /// Creates a TempObject handle for a key without writing any content.
    /// The caller is responsible for writing data to the temp path.
    /// Useful for streaming writes where data is written incrementally.
    #[instrument(skip(self), fields(key = %key))]
    pub async fn create_temp_object(&self, key: &str) -> Result<TempObject> {
        let id = self.layout.object_id(key)?;
        let temp_path = self.layout.temp_path_from_id(&id);
        let final_path = self.layout.object_path_from_id(&id);

        let temp_parent = temp_path
            .parent()
            .ok_or_else(|| StorageError::InvalidPath("Temp path has no parent".to_string()))?;
        fs::create_dir_all(temp_parent).await?;

        debug!("Created temp object handle");

        Ok(TempObject::new(temp_path, final_path))
    }

    /// Writes object data to temporary file and syncs to disk.
    /// Returns handle that auto-cleans up on drop unless committed.
    ///
    /// For atomic operations: call this, then commit_object_metadata, then commit_object.
    #[instrument(skip(self, reader), fields(key = %key))]
    pub async fn write_temp_object<R>(&self, key: &str, mut reader: R) -> Result<TempObject>
    where
        R: AsyncRead + Unpin,
    {
        let temp_object = self.create_temp_object(key).await?;
        let temp_path = temp_object.temp_path();

        debug!(temp_path = ?temp_path, "Writing object to temp path");

        let mut temp_file = fs::File::create(temp_path).await?;
        debug!(temp_path = ?temp_path, "File created");

        #[cfg(feature = "failpoints")]
        fail::fail_point!("storage_write_during_copy");

        let bytes_written = tokio::io::copy(&mut reader, &mut temp_file).await?;
        debug!(bytes_written = bytes_written, "Data copied to temp file");

        temp_file.flush().await?;

        if self.fsync_mode != "none" {
            temp_file.sync_all().await?;
        }
        drop(temp_file);

        debug!(
            temp_path = ?temp_path,
            exists = temp_path.exists(),
            "Object written to temp and synced"
        );

        Ok(temp_object)
    }

    /// Commits temporary object by renaming to final path and syncing parent directory.
    #[instrument(skip(self, temp_object))]
    pub async fn commit_object(&self, mut temp_object: TempObject) -> Result<()> {
        let temp_path = &temp_object.temp_path;
        let final_path = &temp_object.final_path;

        debug!("Committing object");

        #[cfg(feature = "failpoints")]
        fail::fail_point!("storage_commit_before_rename");

        let final_parent = final_path
            .parent()
            .ok_or_else(|| StorageError::InvalidPath("Final path has no parent".to_string()))?;
        fs::create_dir_all(final_parent).await?;

        fs::rename(temp_path, final_path).await?;

        if self.fsync_mode == "full" {
            fsync_dir(final_parent).await?;
        }

        #[cfg(feature = "failpoints")]
        fail::fail_point!("storage_commit_after_fsync");

        temp_object.mark_committed();

        debug!("Object committed");

        Ok(())
    }

    /// Non-atomic write. For atomic operations use write_temp_object + commit_object.
    pub async fn put_object<R>(&self, key: &str, mut reader: R) -> Result<()>
    where
        R: AsyncRead + Unpin,
    {
        let temp_object = self.write_temp_object(key, &mut reader).await?;
        self.commit_object(temp_object).await
    }

    pub async fn get_object(&self, key: &str) -> Result<fs::File> {
        let path = self.layout.object_path(key)?;

        debug!("Reading object {} from path {:?}", key, path);

        match fs::File::open(&path).await {
            Ok(file) => Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(key.to_string()))
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    pub async fn delete_object(&self, key: &str) -> Result<()> {
        let path = self.layout.object_path(key)?;

        debug!("Deleting object {} at path {:?}", key, path);

        match fs::remove_file(&path).await {
            Ok(_) => {
                debug!("Object {} deleted successfully", key);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(key.to_string()))
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.layout.temp_dir()
    }

    /// Get object info (size, checksum) without buffering entire file in memory.
    pub async fn object_info(&self, key: &str) -> Result<(u64, String)> {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncReadExt;

        let path = self.layout.object_path(key)?;

        let metadata = fs::metadata(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::NotFound(key.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;

        let size = metadata.len();

        // Stream-compute checksum to avoid buffering large files
        let mut file = fs::File::open(&path).await.map_err(StorageError::Io)?;
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];

        loop {
            let n = file.read(&mut buf).await.map_err(StorageError::Io)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }

        let checksum = hex::encode(hasher.finalize());
        Ok((size, checksum))
    }
}

#[async_trait]
impl ReplicationStorage for ObjectStorage {
    async fn put_object(&self, key: &str, data: &[u8]) -> Result<()> {
        ObjectStorage::put_object(self, key, data).await
    }

    async fn create_temp_object(&self, key: &str) -> Result<TempObject> {
        ObjectStorage::create_temp_object(self, key).await
    }

    async fn write_temp_object(&self, key: &str, data: &[u8]) -> Result<TempObject> {
        ObjectStorage::write_temp_object(self, key, data).await
    }

    async fn commit_object(&self, temp_object: TempObject) -> Result<()> {
        ObjectStorage::commit_object(self, temp_object).await
    }

    async fn get_object(&self, key: &str) -> Result<fs::File> {
        ObjectStorage::get_object(self, key).await
    }

    async fn object_info(&self, key: &str) -> Result<(u64, String)> {
        ObjectStorage::object_info(self, key).await
    }

    async fn delete_object(&self, key: &str) -> Result<()> {
        ObjectStorage::delete_object(self, key).await
    }
}

pub async fn remove_if_exists<P: AsRef<Path>>(path: P) -> Result<bool> {
    match fs::remove_file(path.as_ref()).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(StorageError::Io(e)),
    }
}

pub async fn list_temp_files<P: AsRef<Path>>(temp_dir: P) -> Result<Vec<(PathBuf, SystemTime)>> {
    let temp_dir = temp_dir.as_ref();

    if !temp_dir.exists() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    let mut dirs_to_process = vec![temp_dir.to_path_buf()];

    while let Some(dir) = dirs_to_process.pop() {
        let mut entries = fs::read_dir(&dir).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                return StorageError::NotFound(dir.display().to_string());
            }
            StorageError::Io(e)
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(StorageError::Io)? {
            let path = entry.path();
            let metadata = entry.metadata().await.map_err(StorageError::Io)?;

            if metadata.is_file() {
                if let Ok(modified) = metadata.modified() {
                    files.push((path, modified));
                }
            } else if metadata.is_dir() {
                dirs_to_process.push(path);
            }
        }
    }

    Ok(files)
}
