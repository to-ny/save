use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};
use tokio::time::timeout;

/// Error type for lock operations
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Failed to acquire lock within timeout")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, LockError>;

/// RAII guard that releases the read lock on drop
pub struct ReadLockGuard {
    _guard: OwnedRwLockReadGuard<()>,
    manager: Arc<Mutex<HashMap<String, Arc<RwLock<()>>>>>,
    key: String,
}

/// RAII guard that releases the write lock on drop
pub struct WriteLockGuard {
    _guard: OwnedRwLockWriteGuard<()>,
    manager: Arc<Mutex<HashMap<String, Arc<RwLock<()>>>>>,
    key: String,
}

fn cleanup_lock(manager: Arc<Mutex<HashMap<String, Arc<RwLock<()>>>>>, key: String) {
    tokio::spawn(async move {
        let mut map = manager.lock().await;
        if let Some(lock_arc) = map.get(&key)
            && Arc::strong_count(lock_arc) == 1
        {
            map.remove(&key);
        }
    });
}

impl Drop for ReadLockGuard {
    fn drop(&mut self) {
        cleanup_lock(Arc::clone(&self.manager), self.key.clone());
    }
}

impl Drop for WriteLockGuard {
    fn drop(&mut self) {
        cleanup_lock(Arc::clone(&self.manager), self.key.clone());
    }
}

pub struct ObjectLockManager {
    locks: Arc<Mutex<HashMap<String, Arc<RwLock<()>>>>>,
    timeout: Duration,
}

impl ObjectLockManager {
    pub fn new(timeout: Duration) -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            timeout,
        }
    }

    pub fn new_default() -> Self {
        Self::new(Duration::from_secs(30))
    }

    pub async fn acquire_read_lock(&self, bucket: &str, key: &str) -> Result<ReadLockGuard> {
        let full_key = format!("{}/{}", bucket, key);

        let lock_arc = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(full_key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };

        let guard = timeout(self.timeout, lock_arc.clone().read_owned())
            .await
            .map_err(|_| LockError::Timeout)?;

        Ok(ReadLockGuard {
            _guard: guard,
            manager: Arc::clone(&self.locks),
            key: full_key,
        })
    }

    pub async fn acquire_write_lock(&self, bucket: &str, key: &str) -> Result<WriteLockGuard> {
        let full_key = format!("{}/{}", bucket, key);

        let lock_arc = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(full_key.clone())
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };

        let guard = timeout(self.timeout, lock_arc.clone().write_owned())
            .await
            .map_err(|_| LockError::Timeout)?;

        Ok(WriteLockGuard {
            _guard: guard,
            manager: Arc::clone(&self.locks),
            key: full_key,
        })
    }

    /// Returns the number of active locks (for testing/metrics)
    #[cfg(test)]
    pub async fn active_lock_count(&self) -> usize {
        let locks = self.locks.lock().await;
        locks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_write_lock_acquire_and_release() {
        let manager = ObjectLockManager::new_default();

        {
            let _guard = manager.acquire_write_lock("bucket", "key").await.unwrap();
            assert_eq!(manager.active_lock_count().await, 1);
        }

        sleep(Duration::from_millis(10)).await;
        assert_eq!(manager.active_lock_count().await, 0);
    }

    #[tokio::test]
    async fn test_concurrent_writes_serialize() {
        let manager = Arc::new(ObjectLockManager::new_default());
        let counter = Arc::new(AtomicU32::new(0));

        let mut handles = vec![];

        for _ in 0..10 {
            let manager = Arc::clone(&manager);
            let counter = Arc::clone(&counter);

            handles.push(tokio::spawn(async move {
                let _guard = manager.acquire_write_lock("bucket", "key").await.unwrap();

                let old = counter.load(Ordering::SeqCst);
                sleep(Duration::from_millis(5)).await;
                counter.store(old + 1, Ordering::SeqCst);
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_different_keys_no_contention() {
        let manager = Arc::new(ObjectLockManager::new_default());
        let counter = Arc::new(AtomicU32::new(0));

        let mut handles = vec![];

        for i in 0..10 {
            let manager = Arc::clone(&manager);
            let counter = Arc::clone(&counter);

            handles.push(tokio::spawn(async move {
                let key = format!("key{}", i);
                let _guard = manager.acquire_write_lock("bucket", &key).await.unwrap();

                counter.fetch_add(1, Ordering::SeqCst);
                sleep(Duration::from_millis(10)).await;
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_write_lock_timeout() {
        let manager = Arc::new(ObjectLockManager::new(Duration::from_millis(100)));

        let _guard = manager.acquire_write_lock("bucket", "key").await.unwrap();

        let manager2 = Arc::clone(&manager);
        let handle =
            tokio::spawn(async move { manager2.acquire_write_lock("bucket", "key").await });

        let result = handle.await.unwrap();
        assert!(matches!(result, Err(LockError::Timeout)));
    }

    #[tokio::test]
    async fn test_lock_cleanup() {
        let manager = ObjectLockManager::new_default();

        for i in 0..5 {
            let key = format!("key{}", i);
            let _guard = manager.acquire_write_lock("bucket", &key).await.unwrap();
        }

        sleep(Duration::from_millis(50)).await;

        assert_eq!(manager.active_lock_count().await, 0);
    }

    #[tokio::test]
    async fn test_same_bucket_different_keys() {
        let manager = Arc::new(ObjectLockManager::new_default());

        let _guard1 = manager.acquire_write_lock("bucket", "key1").await.unwrap();
        let _guard2 = manager.acquire_write_lock("bucket", "key2").await.unwrap();

        assert_eq!(manager.active_lock_count().await, 2);
    }

    #[tokio::test]
    async fn test_different_buckets_same_key() {
        let manager = Arc::new(ObjectLockManager::new_default());

        let _guard1 = manager.acquire_write_lock("bucket1", "key").await.unwrap();
        let _guard2 = manager.acquire_write_lock("bucket2", "key").await.unwrap();

        assert_eq!(manager.active_lock_count().await, 2);
    }

    #[tokio::test]
    async fn test_concurrent_reads_allowed() {
        let manager = Arc::new(ObjectLockManager::new_default());
        let counter = Arc::new(AtomicU32::new(0));

        let mut handles = vec![];

        for _ in 0..10 {
            let manager = Arc::clone(&manager);
            let counter = Arc::clone(&counter);

            handles.push(tokio::spawn(async move {
                let _guard = manager.acquire_read_lock("bucket", "key").await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                sleep(Duration::from_millis(50)).await;
            }));
        }

        sleep(Duration::from_millis(10)).await;

        let count = counter.load(Ordering::SeqCst);
        assert!(count >= 5, "Expected concurrent readers, got {}", count);

        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_write_blocks_reads() {
        let manager = Arc::new(ObjectLockManager::new(Duration::from_millis(100)));

        let _write_guard = manager.acquire_write_lock("bucket", "key").await.unwrap();

        let manager2 = Arc::clone(&manager);
        let handle = tokio::spawn(async move { manager2.acquire_read_lock("bucket", "key").await });

        let result = handle.await.unwrap();
        assert!(matches!(result, Err(LockError::Timeout)));
    }

    #[tokio::test]
    async fn test_read_blocks_write() {
        let manager = Arc::new(ObjectLockManager::new(Duration::from_millis(100)));

        let _read_guard = manager.acquire_read_lock("bucket", "key").await.unwrap();

        let manager2 = Arc::clone(&manager);
        let handle =
            tokio::spawn(async move { manager2.acquire_write_lock("bucket", "key").await });

        let result = handle.await.unwrap();
        assert!(matches!(result, Err(LockError::Timeout)));
    }
}
