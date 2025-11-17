mod error;
mod layout;

#[cfg(test)]
mod tests;

pub use error::{Result, StorageError};

use layout::StorageLayout;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs;
use tokio::io::{AsyncRead, AsyncWriteExt};
use tracing::{debug, instrument, warn};

/// Opaque handle to a temporary object with automatic cleanup on drop.
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

    /// Writes object data to temporary file and syncs to disk.
    /// Returns handle that auto-cleans up on drop unless committed.
    ///
    /// For atomic operations: call this, then commit_object_metadata, then commit_object.
    #[instrument(skip(self, reader), fields(key = %key))]
    pub async fn write_temp_object<R>(&self, key: &str, mut reader: R) -> Result<TempObject>
    where
        R: AsyncRead + Unpin,
    {
        let id = self.layout.object_id(key)?;
        let temp_path = self.layout.temp_path_from_id(&id);
        let final_path = self.layout.object_path_from_id(&id);

        debug!("Writing object to temp path");

        let temp_parent = temp_path
            .parent()
            .ok_or_else(|| StorageError::InvalidPath("Temp path has no parent".to_string()))?;
        fs::create_dir_all(temp_parent).await?;

        let mut temp_file = fs::File::create(&temp_path).await?;

        #[cfg(feature = "failpoints")]
        fail::fail_point!("storage_write_during_copy");

        tokio::io::copy(&mut reader, &mut temp_file).await?;
        temp_file.flush().await?;

        if self.fsync_mode != "none" {
            temp_file.sync_all().await?;
        }
        drop(temp_file);

        debug!("Object written to temp");

        Ok(TempObject::new(temp_path, final_path))
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

        #[cfg(feature = "failpoints")]
        fail::fail_point!("storage_commit_after_rename");

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
