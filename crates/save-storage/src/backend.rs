use crate::error::Result;
use async_trait::async_trait;
use std::any::Any;
use std::fmt::Debug;
use std::path::Path;
use tokio::io::AsyncRead;

/// Temporary object handle with automatic cleanup on drop.
///
/// # Contract
/// A `TempHandle` returned by `StorageBackend::write_temp_object` must only be
/// passed to `commit_object` on the **same backend instance** that created it.
/// Passing a handle to a different backend will result in an error.
pub trait TempHandle: Debug + Send {
    fn temp_path(&self) -> &Path;
    fn as_any(&self) -> &dyn Any;
    fn into_any(self: Box<Self>) -> Box<dyn Any>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { reason: String },
}

/// Storage backend abstraction for local, replicated, or erasure-coded storage.
#[async_trait]
pub trait StorageBackend: Debug + Send + Sync {
    /// Atomic object write.
    async fn put_object(
        &self,
        key: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<()>;

    /// Read object data.
    async fn get_object(&self, key: &str) -> Result<Box<dyn AsyncRead + Send + Unpin>>;

    /// Delete object.
    async fn delete_object(&self, key: &str) -> Result<()>;

    /// Write to temporary location. Auto-cleanup on drop unless committed.
    async fn write_temp_object(
        &self,
        key: &str,
        reader: &mut (dyn AsyncRead + Send + Unpin),
    ) -> Result<Box<dyn TempHandle>>;

    /// Commit temporary object to final location.
    async fn commit_object(&self, temp: Box<dyn TempHandle>) -> Result<()>;

    /// Check backend health.
    async fn health_check(&self) -> Result<HealthStatus>;
}
