use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::time::timeout;

/// Error type for lock operations
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Failed to acquire lock within timeout")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, LockError>;

/// RAII guard that releases the object lock on drop
pub struct LockGuard {
    _guard: OwnedMutexGuard<()>,
    manager: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    key: String,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Lock is automatically released when _guard is dropped
        // Optionally clean up the entry from the map if no one else is waiting
        let manager = Arc::clone(&self.manager);
        let key = self.key.clone();
        tokio::spawn(async move {
            let mut map = manager.lock().await;
            // Only remove if the lock has exactly 1 strong reference (ours)
            if let Some(lock_arc) = map.get(&key)
                && Arc::strong_count(lock_arc) == 1
            {
                map.remove(&key);
            }
        });
    }
}

/// Per-object lock manager to serialize concurrent writes.
pub struct ObjectLockManager {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    timeout: Duration,
}

impl ObjectLockManager {
    /// Creates a new lock manager with the specified timeout
    pub fn new(timeout: Duration) -> Self {
        Self {
            locks: Arc::new(Mutex::new(HashMap::new())),
            timeout,
        }
    }

    /// Creates a new lock manager with default 30 second timeout
    pub fn new_default() -> Self {
        Self::new(Duration::from_secs(30))
    }

    /// Acquires a lock for the given bucket and key.
    /// Returns a guard that releases the lock on drop.
    pub async fn acquire_lock(&self, bucket: &str, key: &str) -> Result<LockGuard> {
        let full_key = format!("{}/{}", bucket, key);

        // Get or create the lock for this key
        let lock_arc = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(full_key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };

        // Acquire the lock with timeout
        let guard = timeout(self.timeout, lock_arc.clone().lock_owned())
            .await
            .map_err(|_| LockError::Timeout)?;

        Ok(LockGuard {
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
    async fn test_lock_acquire_and_release() {
        let manager = ObjectLockManager::new_default();

        {
            let _guard = manager.acquire_lock("bucket", "key").await.unwrap();
            assert_eq!(manager.active_lock_count().await, 1);
        }

        // Give time for cleanup task to run
        sleep(Duration::from_millis(10)).await;
        assert_eq!(manager.active_lock_count().await, 0);
    }

    #[tokio::test]
    async fn test_concurrent_locks_serialize() {
        let manager = Arc::new(ObjectLockManager::new_default());
        let counter = Arc::new(AtomicU32::new(0));

        let mut handles = vec![];

        for _ in 0..10 {
            let manager = Arc::clone(&manager);
            let counter = Arc::clone(&counter);

            handles.push(tokio::spawn(async move {
                let _guard = manager.acquire_lock("bucket", "key").await.unwrap();

                // Critical section - increment counter
                let old = counter.load(Ordering::SeqCst);
                sleep(Duration::from_millis(5)).await;
                counter.store(old + 1, Ordering::SeqCst);
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // If locks work correctly, counter should be exactly 10
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_different_keys_no_contention() {
        let manager = Arc::new(ObjectLockManager::new_default());
        let counter = Arc::new(AtomicU32::new(0));

        let mut handles = vec![];

        // Different keys should not block each other
        for i in 0..10 {
            let manager = Arc::clone(&manager);
            let counter = Arc::clone(&counter);

            handles.push(tokio::spawn(async move {
                let key = format!("key{}", i);
                let _guard = manager.acquire_lock("bucket", &key).await.unwrap();

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
    async fn test_lock_timeout() {
        let manager = Arc::new(ObjectLockManager::new(Duration::from_millis(100)));

        // Hold the lock
        let _guard = manager.acquire_lock("bucket", "key").await.unwrap();

        // Try to acquire from another task - should timeout
        let manager2 = Arc::clone(&manager);
        let handle = tokio::spawn(async move { manager2.acquire_lock("bucket", "key").await });

        let result = handle.await.unwrap();
        assert!(matches!(result, Err(LockError::Timeout)));
    }

    #[tokio::test]
    async fn test_lock_cleanup() {
        let manager = ObjectLockManager::new_default();

        // Acquire and release locks for multiple keys
        for i in 0..5 {
            let key = format!("key{}", i);
            let _guard = manager.acquire_lock("bucket", &key).await.unwrap();
            // Guard dropped here
        }

        // Give cleanup tasks time to run
        sleep(Duration::from_millis(50)).await;

        // All locks should be cleaned up
        assert_eq!(manager.active_lock_count().await, 0);
    }

    #[tokio::test]
    async fn test_same_bucket_different_keys() {
        let manager = Arc::new(ObjectLockManager::new_default());

        let _guard1 = manager.acquire_lock("bucket", "key1").await.unwrap();
        let _guard2 = manager.acquire_lock("bucket", "key2").await.unwrap();

        assert_eq!(manager.active_lock_count().await, 2);
    }

    #[tokio::test]
    async fn test_different_buckets_same_key() {
        let manager = Arc::new(ObjectLockManager::new_default());

        let _guard1 = manager.acquire_lock("bucket1", "key").await.unwrap();
        let _guard2 = manager.acquire_lock("bucket2", "key").await.unwrap();

        assert_eq!(manager.active_lock_count().await, 2);
    }
}
