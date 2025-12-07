use crate::error::Result;
use crate::raft::{LockHolder, LockType, RaftNode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::warn;

fn lock_key(bucket: &str, key: &str) -> String {
    format!("lock:{}/{}", bucket, key)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockState {
    pub lock_type: LockType,
    pub holders: Vec<LockHolder>,
}

impl LockState {
    pub fn can_acquire(&self, lock_type: LockType, holder: &LockHolder) -> bool {
        if self.holders.iter().any(|h| h == holder) {
            return true; // Already holding
        }
        match (self.lock_type, lock_type) {
            (LockType::Read, LockType::Read) => true,
            (LockType::Read, LockType::Write) => false,
            (LockType::Write, _) => false,
        }
    }

    pub fn is_held_by(&self, holder: &LockHolder) -> bool {
        self.holders.iter().any(|h| h == holder)
    }
}

pub(crate) fn get_lock_state(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
) -> Result<Option<LockState>> {
    let db_key = lock_key(bucket, key);
    match db.get(&db_key)? {
        Some(data) => {
            let (state, _): (LockState, _) =
                bincode::serde::decode_from_slice(&data, bincode::config::standard())?;
            Ok(Some(state))
        }
        None => Ok(None),
    }
}

pub(crate) fn acquire_lock(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    lock_type: LockType,
    holder: &LockHolder,
) -> Result<bool> {
    let db_key = lock_key(bucket, key);

    let current_state = get_lock_state(db, bucket, key)?;

    match current_state {
        None => {
            let state = LockState {
                lock_type,
                holders: vec![holder.clone()],
            };
            let value = bincode::serde::encode_to_vec(&state, bincode::config::standard())?;
            db.put(&db_key, value)?;
            Ok(true)
        }
        Some(mut state) => {
            if state.is_held_by(holder) {
                return Ok(true); // Already holding
            }
            if !state.can_acquire(lock_type, holder) {
                return Ok(false);
            }
            state.holders.push(holder.clone());
            let value = bincode::serde::encode_to_vec(&state, bincode::config::standard())?;
            db.put(&db_key, value)?;
            Ok(true)
        }
    }
}

pub(crate) fn release_lock(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    holder: &LockHolder,
) -> Result<()> {
    let db_key = lock_key(bucket, key);

    if let Some(mut state) = get_lock_state(db, bucket, key)? {
        state.holders.retain(|h| h != holder);
        if state.holders.is_empty() {
            db.delete(&db_key)?;
        } else {
            let value = bincode::serde::encode_to_vec(&state, bincode::config::standard())?;
            db.put(&db_key, value)?;
        }
    }
    Ok(())
}

// --- Distributed Lock Manager ---

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("Failed to acquire lock within timeout")]
    Timeout,
    #[error("Lock acquisition failed: {0}")]
    AcquisitionFailed(String),
}

pub type LockResult<T> = std::result::Result<T, LockError>;

struct DistributedLockGuard {
    raft_node: Arc<RaftNode>,
    bucket: String,
    key: String,
    holder: LockHolder,
}

impl Drop for DistributedLockGuard {
    fn drop(&mut self) {
        let raft_node = Arc::clone(&self.raft_node);
        let bucket = self.bucket.clone();
        let key = self.key.clone();
        let holder = self.holder.clone();
        tokio::spawn(async move {
            if let Err(e) = raft_node.release_lock(&bucket, &key, holder).await {
                warn!(
                    bucket = %bucket,
                    key = %key,
                    error = %e,
                    "Failed to release distributed lock"
                );
            }
        });
    }
}

/// RAII guard for a distributed read lock. Lock is released when dropped.
pub struct DistributedReadLockGuard(#[allow(dead_code)] DistributedLockGuard);

/// RAII guard for a distributed write lock. Lock is released when dropped.
pub struct DistributedWriteLockGuard(#[allow(dead_code)] DistributedLockGuard);

pub struct DistributedLockManager {
    raft_node: Arc<RaftNode>,
    db: Arc<rocksdb::DB>,
    node_id: u64,
    lock_counter: AtomicU64,
    timeout: Duration,
    retry_interval: Duration,
}

impl DistributedLockManager {
    pub fn new(raft_node: Arc<RaftNode>, db: Arc<rocksdb::DB>, node_id: u64) -> Self {
        Self {
            raft_node,
            db,
            node_id,
            lock_counter: AtomicU64::new(0),
            timeout: Duration::from_secs(30),
            retry_interval: Duration::from_millis(50),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn next_holder(&self) -> LockHolder {
        LockHolder {
            node_id: self.node_id,
            lock_id: self.lock_counter.fetch_add(1, Ordering::SeqCst),
        }
    }

    fn can_acquire_locally(
        &self,
        bucket: &str,
        key: &str,
        lock_type: LockType,
        holder: &LockHolder,
    ) -> LockResult<bool> {
        let state = get_lock_state(&self.db, bucket, key)
            .map_err(|e| LockError::AcquisitionFailed(e.to_string()))?;

        match state {
            None => Ok(true),
            Some(s) => Ok(s.can_acquire(lock_type, holder)),
        }
    }

    async fn try_acquire_lock(
        &self,
        bucket: &str,
        key: &str,
        lock_type: LockType,
        holder: &LockHolder,
    ) -> LockResult<bool> {
        // Check local state first to avoid unnecessary Raft round-trips
        if !self.can_acquire_locally(bucket, key, lock_type, holder)? {
            return Ok(false);
        }

        self.raft_node
            .acquire_lock(bucket, key, lock_type, holder.clone())
            .await
            .map_err(|e| LockError::AcquisitionFailed(e.to_string()))?;

        // Verify the lock was actually acquired
        let state = get_lock_state(&self.db, bucket, key)
            .map_err(|e| LockError::AcquisitionFailed(e.to_string()))?;

        Ok(state.is_some_and(|s| s.is_held_by(holder)))
    }

    async fn acquire_lock_internal(
        &self,
        bucket: &str,
        key: &str,
        lock_type: LockType,
    ) -> LockResult<DistributedLockGuard> {
        let holder = self.next_holder();

        let acquire_future = async {
            loop {
                if self
                    .try_acquire_lock(bucket, key, lock_type, &holder)
                    .await?
                {
                    return Ok(DistributedLockGuard {
                        raft_node: Arc::clone(&self.raft_node),
                        bucket: bucket.to_string(),
                        key: key.to_string(),
                        holder,
                    });
                }
                sleep(self.retry_interval).await;
            }
        };

        timeout(self.timeout, acquire_future)
            .await
            .map_err(|_| LockError::Timeout)?
    }

    pub async fn acquire_read_lock(
        &self,
        bucket: &str,
        key: &str,
    ) -> LockResult<DistributedReadLockGuard> {
        self.acquire_lock_internal(bucket, key, LockType::Read)
            .await
            .map(DistributedReadLockGuard)
    }

    pub async fn acquire_write_lock(
        &self,
        bucket: &str,
        key: &str,
    ) -> LockResult<DistributedWriteLockGuard> {
        self.acquire_lock_internal(bucket, key, LockType::Write)
            .await
            .map(DistributedWriteLockGuard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> rocksdb::DB {
        let path = tempfile::tempdir().unwrap();
        rocksdb::DB::open_default(path.path()).unwrap()
    }

    #[test]
    fn test_acquire_write_lock() {
        let db = create_test_db();
        let holder = LockHolder {
            node_id: 1,
            lock_id: 100,
        };

        let result = acquire_lock(&db, "bucket", "key", LockType::Write, &holder).unwrap();
        assert!(result);

        let state = get_lock_state(&db, "bucket", "key").unwrap().unwrap();
        assert!(state.is_held_by(&holder));
    }

    #[test]
    fn test_write_lock_blocks_write() {
        let db = create_test_db();
        let holder1 = LockHolder {
            node_id: 1,
            lock_id: 100,
        };
        let holder2 = LockHolder {
            node_id: 2,
            lock_id: 200,
        };

        assert!(acquire_lock(&db, "bucket", "key", LockType::Write, &holder1).unwrap());
        assert!(!acquire_lock(&db, "bucket", "key", LockType::Write, &holder2).unwrap());
    }

    #[test]
    fn test_write_lock_blocks_read() {
        let db = create_test_db();
        let holder1 = LockHolder {
            node_id: 1,
            lock_id: 100,
        };
        let holder2 = LockHolder {
            node_id: 2,
            lock_id: 200,
        };

        assert!(acquire_lock(&db, "bucket", "key", LockType::Write, &holder1).unwrap());
        assert!(!acquire_lock(&db, "bucket", "key", LockType::Read, &holder2).unwrap());
    }

    #[test]
    fn test_read_locks_concurrent() {
        let db = create_test_db();
        let holder1 = LockHolder {
            node_id: 1,
            lock_id: 100,
        };
        let holder2 = LockHolder {
            node_id: 2,
            lock_id: 200,
        };

        assert!(acquire_lock(&db, "bucket", "key", LockType::Read, &holder1).unwrap());
        assert!(acquire_lock(&db, "bucket", "key", LockType::Read, &holder2).unwrap());

        let state = get_lock_state(&db, "bucket", "key").unwrap().unwrap();
        assert_eq!(state.holders.len(), 2);
    }

    #[test]
    fn test_read_lock_blocks_write() {
        let db = create_test_db();
        let holder1 = LockHolder {
            node_id: 1,
            lock_id: 100,
        };
        let holder2 = LockHolder {
            node_id: 2,
            lock_id: 200,
        };

        assert!(acquire_lock(&db, "bucket", "key", LockType::Read, &holder1).unwrap());
        assert!(!acquire_lock(&db, "bucket", "key", LockType::Write, &holder2).unwrap());
    }

    #[test]
    fn test_release_lock() {
        let db = create_test_db();
        let holder = LockHolder {
            node_id: 1,
            lock_id: 100,
        };

        acquire_lock(&db, "bucket", "key", LockType::Write, &holder).unwrap();
        release_lock(&db, "bucket", "key", &holder).unwrap();

        assert!(get_lock_state(&db, "bucket", "key").unwrap().is_none());
    }

    #[test]
    fn test_release_one_reader() {
        let db = create_test_db();
        let holder1 = LockHolder {
            node_id: 1,
            lock_id: 100,
        };
        let holder2 = LockHolder {
            node_id: 2,
            lock_id: 200,
        };

        acquire_lock(&db, "bucket", "key", LockType::Read, &holder1).unwrap();
        acquire_lock(&db, "bucket", "key", LockType::Read, &holder2).unwrap();
        release_lock(&db, "bucket", "key", &holder1).unwrap();

        let state = get_lock_state(&db, "bucket", "key").unwrap().unwrap();
        assert_eq!(state.holders.len(), 1);
        assert!(state.is_held_by(&holder2));
    }

    #[test]
    fn test_idempotent_acquire() {
        let db = create_test_db();
        let holder = LockHolder {
            node_id: 1,
            lock_id: 100,
        };

        assert!(acquire_lock(&db, "bucket", "key", LockType::Write, &holder).unwrap());
        assert!(acquire_lock(&db, "bucket", "key", LockType::Write, &holder).unwrap());

        let state = get_lock_state(&db, "bucket", "key").unwrap().unwrap();
        assert_eq!(state.holders.len(), 1);
    }
}
