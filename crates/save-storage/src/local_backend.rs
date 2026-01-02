use crate::ObjectStorage;
use crate::backend::{HealthStatus, StorageBackend, TempHandle};
use crate::error::Result;
use async_trait::async_trait;
use std::any::Any;
use std::path::{Path, PathBuf};
use tokio::io::AsyncRead;

/// TempHandle wrapper for TempObject.
#[derive(Debug)]
pub struct LocalTempHandle {
    inner: crate::TempObject,
}

impl LocalTempHandle {
    fn new(temp_object: crate::TempObject) -> Self {
        Self { inner: temp_object }
    }

    fn into_inner(self) -> crate::TempObject {
        self.inner
    }
}

impl TempHandle for LocalTempHandle {
    fn temp_path(&self) -> &Path {
        self.inner.temp_path()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

/// Local filesystem storage backend wrapping ObjectStorage.
#[derive(Debug)]
pub struct LocalBackend {
    storage: ObjectStorage,
}

impl LocalBackend {
    pub async fn new<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        Self::new_with_fsync_mode(base_path, "data").await
    }

    pub async fn new_with_fsync_mode<P: AsRef<Path>>(
        base_path: P,
        fsync_mode: &str,
    ) -> Result<Self> {
        let storage = ObjectStorage::new_with_fsync_mode(base_path, fsync_mode).await?;
        Ok(Self { storage })
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.storage.temp_dir()
    }
}

#[async_trait]
impl StorageBackend for LocalBackend {
    async fn put_object(
        &self,
        key: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<()> {
        self.storage.put_object(key, reader).await
    }

    async fn get_object(&self, key: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        let file = self.storage.get_object(key).await?;
        Ok(Box::new(file) as Box<dyn AsyncRead + Send + Unpin>)
    }

    async fn delete_object(&self, key: &str) -> Result<()> {
        self.storage.delete_object(key).await
    }

    async fn write_temp_object(
        &self,
        key: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<Box<dyn TempHandle>> {
        let temp_object = self.storage.write_temp_object(key, reader).await?;
        Ok(Box::new(LocalTempHandle::new(temp_object)) as Box<dyn TempHandle>)
    }

    async fn commit_object(&self, temp: Box<dyn TempHandle>) -> Result<()> {
        let temp_any = temp.into_any();
        let temp_handle = temp_any.downcast::<LocalTempHandle>().map_err(|_| {
            crate::error::StorageError::Io(std::io::Error::other(
                "TempHandle must be LocalTempHandle for LocalBackend",
            ))
        })?;

        let temp_object = temp_handle.into_inner();
        self.storage.commit_object(temp_object).await
    }

    async fn health_check(&self) -> Result<HealthStatus> {
        let temp_dir = self.storage.temp_dir();

        match tokio::fs::metadata(&temp_dir).await {
            Ok(metadata) if metadata.is_dir() => Ok(HealthStatus::Healthy),
            Ok(_) => Ok(HealthStatus::Unhealthy {
                reason: "Temp directory path exists but is not a directory".to_string(),
            }),
            Err(e) => Ok(HealthStatus::Unhealthy {
                reason: format!("Cannot access temp directory: {}", e),
            }),
        }
    }

    fn temp_dir(&self) -> PathBuf {
        self.storage.temp_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_local_backend_creation() {
        let temp_dir = TempDir::new().unwrap();
        let backend = LocalBackend::new(temp_dir.path()).await.unwrap();
        assert!(backend.temp_dir().exists());
    }

    #[tokio::test]
    async fn test_health_check_healthy() {
        let temp_dir = TempDir::new().unwrap();
        let backend = LocalBackend::new(temp_dir.path()).await.unwrap();
        let health = backend.health_check().await.unwrap();
        assert_eq!(health, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_put_and_get_object() {
        let temp_dir = TempDir::new().unwrap();
        let backend = LocalBackend::new(temp_dir.path()).await.unwrap();

        let key = "test/object.txt";
        let data = b"Hello, World!";
        let mut reader = &data[..];

        backend.put_object(key, &mut reader).await.unwrap();

        let mut file = backend.get_object(key).await.unwrap();
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, data);
    }

    #[tokio::test]
    async fn test_delete_object() {
        let temp_dir = TempDir::new().unwrap();
        let backend = LocalBackend::new(temp_dir.path()).await.unwrap();

        let key = "test/object.txt";
        let data = b"test data";
        let mut reader = &data[..];

        backend.put_object(key, &mut reader).await.unwrap();
        backend.delete_object(key).await.unwrap();

        let result = backend.get_object(key).await;
        assert!(result.is_err());
    }
}
