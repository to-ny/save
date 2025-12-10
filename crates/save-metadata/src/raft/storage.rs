//! RocksDB-backed Raft storage.
//!
//! Implements `RaftLogReader` and `RaftStorage` traits for openraft.

use super::log_store::LogStore;
use super::snapshot::SnapshotStorage;
use super::state_machine::apply_command;
use super::types::{Entry, LogId, NodeId, NodeTypeConfig, Vote};
use openraft::{
    LogState, RaftLogReader, RaftStorage, Snapshot, SnapshotMeta, StorageError, StorageIOError,
    StoredMembership,
};
use std::ops::RangeBounds;
use std::sync::Arc;

/// RocksDB-backed Raft storage.
///
/// Requires column families: raft_log, raft_state, raft_snapshot.
pub struct Storage {
    log_store: LogStore,
}

impl Storage {
    pub fn new(db: Arc<rocksdb::DB>) -> Self {
        Self {
            log_store: LogStore::new(db),
        }
    }

    fn db(&self) -> &Arc<rocksdb::DB> {
        self.log_store.db()
    }

    fn state_cf(&self) -> Result<&rocksdb::ColumnFamily, StorageError<NodeId>> {
        self.db().cf_handle("raft_state").ok_or_else(|| {
            StorageIOError::read(&std::io::Error::other("raft_state CF not found")).into()
        })
    }
}

impl RaftLogReader<NodeTypeConfig> for Storage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + std::fmt::Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<NodeId>> {
        self.log_store.get_entries(range)
    }
}

impl RaftStorage<NodeTypeConfig> for Storage {
    type LogReader = Self;
    type SnapshotBuilder = SnapshotStorage;

    async fn save_vote(&mut self, vote: &Vote) -> Result<(), StorageError<NodeId>> {
        let cf = self.state_cf()?;
        let encoded = serde_json::to_vec(vote)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        self.db()
            .put_cf(cf, b"vote", encoded)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote>, StorageError<NodeId>> {
        let cf = self.state_cf()?;
        let data = self
            .db()
            .get_cf(cf, b"vote")
            .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?;

        match data {
            Some(bytes) => {
                let vote = serde_json::from_slice(&bytes)
                    .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?;
                Ok(Some(vote))
            }
            None => Ok(None),
        }
    }

    async fn get_log_state(&mut self) -> Result<LogState<NodeTypeConfig>, StorageError<NodeId>> {
        self.log_store.get_log_state()
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        Storage::new(Arc::clone(self.db()))
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry> + Send,
    {
        self.log_store.append(entries)?;
        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId,
    ) -> Result<(), StorageError<NodeId>> {
        let start_index = log_id.index;
        let current_state = self.log_store.get_log_state()?;

        if let Some(last_log_id) = current_state.last_log_id {
            self.log_store
                .delete_range(start_index, last_log_id.index)?;

            if start_index > 0 {
                let new_last = LogId::new(log_id.leader_id, start_index - 1);
                self.log_store.write_log_id(b"last_log_id", &new_last)?;
            } else {
                self.log_store.delete_key(b"last_log_id")?;
            }
        }

        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId) -> Result<(), StorageError<NodeId>> {
        let current_state = self.log_store.get_log_state()?;
        let start_index = current_state
            .last_purged_log_id
            .map(|id| id.index + 1)
            .unwrap_or(0);

        self.log_store.delete_range(start_index, log_id.index)?;
        self.log_store
            .write_log_id(b"last_purged_log_id", &log_id)?;

        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId>, StoredMembership<NodeId, openraft::BasicNode>), StorageError<NodeId>>
    {
        let last_applied = self.log_store.read_log_id(b"last_applied_log_id")?;

        let cf = self.state_cf()?;
        let membership = match self
            .db()
            .get_cf(cf, b"membership")
            .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?
        {
            Some(data) => serde_json::from_slice(&data)
                .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?,
            None => StoredMembership::default(),
        };

        Ok((last_applied, membership))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry],
    ) -> Result<Vec<()>, StorageError<NodeId>> {
        let mut responses = Vec::new();
        let mut last_applied = None;

        for entry in entries {
            match &entry.payload {
                openraft::EntryPayload::Blank => {}
                openraft::EntryPayload::Normal(cmd) => {
                    apply_command(self.db(), cmd)?;
                }
                openraft::EntryPayload::Membership(membership) => {
                    let cf = self.state_cf()?;
                    let stored = StoredMembership::new(Some(entry.log_id), membership.clone());
                    let encoded = serde_json::to_vec(&stored).map_err(|e| {
                        StorageIOError::write(&std::io::Error::other(e.to_string()))
                    })?;

                    self.db().put_cf(cf, b"membership", encoded).map_err(|e| {
                        StorageIOError::write(&std::io::Error::other(e.to_string()))
                    })?;
                }
            }

            last_applied = Some(entry.log_id);
            responses.push(());
        }

        if let Some(log_id) = last_applied {
            self.log_store
                .write_log_id(b"last_applied_log_id", &log_id)?;
        }

        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        SnapshotStorage::new(Arc::clone(self.db()))
    }

    async fn begin_receiving_snapshot(
        &mut self,
    ) -> Result<Box<std::io::Cursor<Vec<u8>>>, StorageError<NodeId>> {
        Ok(Box::new(std::io::Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, openraft::BasicNode>,
        snapshot: Box<std::io::Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        let db = Arc::clone(self.db());
        let meta = meta.clone();
        tokio::task::spawn_blocking(move || super::snapshot::install_snapshot(&db, &meta, snapshot))
            .await
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<NodeTypeConfig>>, StorageError<NodeId>> {
        let db = Arc::clone(self.db());
        tokio::task::spawn_blocking(move || super::snapshot::get_current_snapshot(&db))
            .await
            .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bucket, ObjectMetadata};
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

    fn create_test_storage() -> (TempDir, Storage) {
        let (temp_dir, db) = create_test_db();
        (temp_dir, Storage::new(db))
    }

    #[tokio::test]
    async fn test_vote_persistence_empty() {
        let (_temp, mut storage) = create_test_storage();
        assert_eq!(storage.read_vote().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_vote_persistence_save_read() {
        let (_temp, mut storage) = create_test_storage();

        let vote = Vote::new(5, 1);
        storage.save_vote(&vote).await.unwrap();

        assert_eq!(storage.read_vote().await.unwrap(), Some(vote));
    }

    #[tokio::test]
    async fn test_vote_persistence_overwrite() {
        let (_temp, mut storage) = create_test_storage();

        storage.save_vote(&Vote::new(5, 1)).await.unwrap();
        storage.save_vote(&Vote::new(10, 2)).await.unwrap();

        assert_eq!(storage.read_vote().await.unwrap(), Some(Vote::new(10, 2)));
    }

    #[tokio::test]
    async fn test_log_state_empty() {
        let (_temp, mut storage) = create_test_storage();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, None);
        assert_eq!(state.last_purged_log_id, None);
    }

    #[tokio::test]
    async fn test_append_and_read_entries() {
        let (_temp, mut storage) = create_test_storage();

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Blank,
            },
        ];

        storage.append_to_log(entries).await.unwrap();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, Some(log_id(1, 1, 2)));

        let read = storage.try_get_log_entries(1..3).await.unwrap();
        assert_eq!(read.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_conflict_logs() {
        let (_temp, mut storage) = create_test_storage();

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

        storage.append_to_log(entries).await.unwrap();
        storage
            .delete_conflict_logs_since(log_id(1, 1, 2))
            .await
            .unwrap();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, Some(log_id(1, 1, 1)));
    }

    #[tokio::test]
    async fn test_purge_logs() {
        let (_temp, mut storage) = create_test_storage();

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Blank,
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Blank,
            },
        ];

        storage.append_to_log(entries).await.unwrap();
        storage.purge_logs_upto(log_id(1, 1, 1)).await.unwrap();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 1)));

        let read = storage.try_get_log_entries(1..3).await.unwrap();
        assert_eq!(read.len(), 1);
    }

    #[tokio::test]
    async fn test_last_applied_state_empty() {
        let (_temp, mut storage) = create_test_storage();

        let (last, membership) = storage.last_applied_state().await.unwrap();
        assert_eq!(last, None);
        assert_eq!(membership, StoredMembership::default());
    }

    #[tokio::test]
    async fn test_apply_create_bucket() {
        let (_temp, mut storage) = create_test_storage();

        let bucket = Bucket {
            name: "test-bucket".to_string(),
            created_at: chrono::Utc::now(),
        };

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Normal(crate::raft::Command::CreateBucket { bucket }),
        };

        storage.apply_to_state_machine(&[entry]).await.unwrap();

        let (last, _) = storage.last_applied_state().await.unwrap();
        assert_eq!(last, Some(log_id(1, 1, 1)));

        let stored = crate::bucket::get_bucket(storage.db(), "test-bucket").unwrap();
        assert_eq!(stored.name, "test-bucket");
    }

    #[tokio::test]
    async fn test_apply_put_object_metadata() {
        let (_temp, mut storage) = create_test_storage();

        crate::bucket::create_bucket(storage.db(), "test-bucket").unwrap();

        let now = chrono::Utc::now();
        let metadata = ObjectMetadata {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            size: 1024,
            etag: "abc123".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: now,
            modified_at: now,
            replica_nodes: vec![1, 2, 3],
        };

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Normal(crate::raft::Command::PutObjectMetadata { metadata }),
        };

        storage.apply_to_state_machine(&[entry]).await.unwrap();

        let stored =
            crate::object::get_object_metadata(storage.db(), "test-bucket", "test-key").unwrap();
        assert_eq!(stored.replica_nodes, vec![1, 2, 3]);
    }
}
