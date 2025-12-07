//! Raft log storage operations.
//!
//! Note: Functions return openraft's `StorageError` which is intentionally large
//! to provide comprehensive error context for distributed consensus debugging.

use super::types::{Entry, LogId, NodeId};
use openraft::{LogState, StorageError, StorageIOError};
use std::ops::RangeBounds;
use std::sync::Arc;

/// Helper for Raft log storage operations.
pub struct LogStore {
    db: Arc<rocksdb::DB>,
}

impl LogStore {
    pub fn new(db: Arc<rocksdb::DB>) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &Arc<rocksdb::DB> {
        &self.db
    }

    fn log_cf(&self) -> Result<&rocksdb::ColumnFamily, StorageError<NodeId>> {
        self.db
            .cf_handle("raft_log")
            .ok_or_else(|| StorageIOError::read(&std::io::Error::other("raft_log CF not found")).into())
    }

    fn state_cf(&self) -> Result<&rocksdb::ColumnFamily, StorageError<NodeId>> {
        self.db
            .cf_handle("raft_state")
            .ok_or_else(|| StorageIOError::read(&std::io::Error::other("raft_state CF not found")).into())
    }

    fn index_to_key(index: u64) -> [u8; 8] {
        index.to_be_bytes()
    }

    pub fn read_log_id(&self, key: &[u8]) -> Result<Option<LogId>, StorageError<NodeId>> {
        let cf = self.state_cf()?;
        let data = self
            .db
            .get_cf(cf, key)
            .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?;

        match data {
            Some(bytes) => {
                let log_id = serde_json::from_slice(&bytes)
                    .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?;
                Ok(Some(log_id))
            }
            None => Ok(None),
        }
    }

    pub fn write_log_id(&self, key: &[u8], log_id: &LogId) -> Result<(), StorageError<NodeId>> {
        let cf = self.state_cf()?;
        let encoded = serde_json::to_vec(log_id)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        self.db
            .put_cf(cf, key, encoded)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    pub fn delete_key(&self, key: &[u8]) -> Result<(), StorageError<NodeId>> {
        let cf = self.state_cf()?;
        self.db
            .delete_cf(cf, key)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    pub fn get_log_state(
        &self,
    ) -> Result<LogState<super::types::NodeTypeConfig>, StorageError<NodeId>> {
        let last_purged_log_id = self.read_log_id(b"last_purged_log_id")?;
        let last_log_id = self.read_log_id(b"last_log_id")?;

        Ok(LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    pub fn get_entries<RB: RangeBounds<u64> + Clone + std::fmt::Debug>(
        &self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<NodeId>> {
        let cf = self.log_cf()?;

        let start = match range.start_bound() {
            std::ops::Bound::Included(&n) => n,
            std::ops::Bound::Excluded(&n) => n + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            std::ops::Bound::Included(&n) => n + 1,
            std::ops::Bound::Excluded(&n) => n,
            std::ops::Bound::Unbounded => u64::MAX,
        };

        let mut entries = Vec::new();
        for index in start..end {
            let key = Self::index_to_key(index);
            if let Some(data) = self
                .db
                .get_cf(cf, key)
                .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?
            {
                let entry = serde_json::from_slice(&data)
                    .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?;
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    pub fn append<I>(&self, entries: I) -> Result<Option<LogId>, StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry>,
    {
        let cf = self.log_cf()?;
        let mut last_log_id = None;

        for entry in entries {
            let index = entry.log_id.index;
            let key = Self::index_to_key(index);

            let encoded = serde_json::to_vec(&entry)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

            self.db
                .put_cf(cf, key, encoded)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

            last_log_id = Some(entry.log_id);
        }

        if let Some(ref log_id) = last_log_id {
            self.write_log_id(b"last_log_id", log_id)?;
        }

        Ok(last_log_id)
    }

    pub fn delete_range(&self, start: u64, end: u64) -> Result<(), StorageError<NodeId>> {
        let cf = self.log_cf()?;

        for index in start..=end {
            let key = Self::index_to_key(index);
            self.db
                .delete_cf(cf, key)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::EntryPayload;
    use openraft::testing::log_id;
    use tempfile::TempDir;

    fn create_test_db() -> (TempDir, Arc<rocksdb::DB>) {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cf_names = ["default", "raft_log", "raft_state", "raft_snapshot"];
        let cf_opts = rocksdb::Options::default();
        let cfs = cf_names
            .iter()
            .map(|name| rocksdb::ColumnFamilyDescriptor::new(*name, cf_opts.clone()));

        let db = rocksdb::DB::open_cf_descriptors(&opts, temp_dir.path(), cfs).unwrap();
        (temp_dir, Arc::new(db))
    }

    #[test]
    fn test_log_state_empty() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let state = store.get_log_state().unwrap();
        assert_eq!(state.last_log_id, None);
        assert_eq!(state.last_purged_log_id, None);
    }

    #[test]
    fn test_append_single_entry() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Blank,
        };

        let last = store.append(vec![entry]).unwrap();
        assert_eq!(last, Some(log_id(1, 1, 1)));

        let state = store.get_log_state().unwrap();
        assert_eq!(state.last_log_id, Some(log_id(1, 1, 1)));
    }

    #[test]
    fn test_append_multiple_entries() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 3),
                payload: EntryPayload::Blank,
            },
        ];

        store.append(entries).unwrap();

        let state = store.get_log_state().unwrap();
        assert_eq!(state.last_log_id, Some(log_id(1, 1, 3)));
    }

    #[test]
    fn test_get_entries() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 3),
                payload: EntryPayload::Blank,
            },
        ];

        store.append(entries).unwrap();

        let read = store.get_entries(1..4).unwrap();
        assert_eq!(read.len(), 3);
        assert_eq!(read[0].log_id, log_id(1, 1, 1));
        assert_eq!(read[2].log_id, log_id(1, 1, 3));
    }

    #[test]
    fn test_get_entries_partial() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 3),
                payload: EntryPayload::Blank,
            },
        ];

        store.append(entries).unwrap();

        let read = store.get_entries(2..3).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].log_id, log_id(1, 1, 2));
    }

    #[test]
    fn test_delete_range() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 3),
                payload: EntryPayload::Blank,
            },
        ];

        store.append(entries).unwrap();
        store.delete_range(2, 3).unwrap();

        let read = store.get_entries(1..4).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].log_id, log_id(1, 1, 1));
    }

    #[test]
    fn test_log_id_persistence() {
        let (_temp, db) = create_test_db();
        let store = LogStore::new(db);

        let log_id = log_id(1, 1, 5);
        store.write_log_id(b"test_key", &log_id).unwrap();

        let read = store.read_log_id(b"test_key").unwrap();
        assert_eq!(read, Some(log_id));
    }
}
