mod error;
mod layout;

#[cfg(test)]
mod tests;

pub use error::{Result, StorageError};

use layout::StorageLayout;
use std::path::{Path, PathBuf};
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
}

impl ObjectStorage {
    pub async fn new<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        let layout = StorageLayout::new(base_path);

        fs::create_dir_all(layout.objects_dir()).await?;
        fs::create_dir_all(layout.temp_dir()).await?;

        Ok(Self { layout })
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
        tokio::io::copy(&mut reader, &mut temp_file).await?;
        temp_file.flush().await?;
        temp_file.sync_all().await?;
        drop(temp_file);

        debug!("Object written to temp and synced");

        Ok(TempObject::new(temp_path, final_path))
    }

    /// Commits temporary object by renaming to final path and syncing parent directory.
    #[instrument(skip(self, temp_object))]
    pub async fn commit_object(&self, mut temp_object: TempObject) -> Result<()> {
        let temp_path = &temp_object.temp_path;
        let final_path = &temp_object.final_path;

        debug!("Committing object");

        let final_parent = final_path
            .parent()
            .ok_or_else(|| StorageError::InvalidPath("Final path has no parent".to_string()))?;
        fs::create_dir_all(final_parent).await?;

        fs::rename(temp_path, final_path).await?;
        fsync_dir(final_parent).await?;

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
}
