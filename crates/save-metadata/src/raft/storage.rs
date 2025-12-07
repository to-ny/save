use super::snapshot::SnapshotStorage;
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
    db: Arc<rocksdb::DB>,
}

impl Storage {
    pub fn new(db: Arc<rocksdb::DB>) -> Self {
        Self { db }
    }

    #[allow(clippy::result_large_err)]
    fn read_log_id(&self, key: &[u8]) -> Result<Option<LogId>, StorageError<NodeId>> {
        let cf = self.db.cf_handle("raft_state").ok_or_else(|| {
            StorageIOError::read(&std::io::Error::other("raft_state CF not found"))
        })?;

        let data = self
            .db
            .get_cf(&cf, key)
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

    #[allow(clippy::result_large_err)]
    fn write_log_id(&self, key: &[u8], log_id: &LogId) -> Result<(), StorageError<NodeId>> {
        let cf = self.db.cf_handle("raft_state").ok_or_else(|| {
            StorageIOError::write(&std::io::Error::other("raft_state CF not found"))
        })?;

        let encoded = serde_json::to_vec(log_id)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        self.db
            .put_cf(&cf, key, encoded)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    fn log_index_to_key(index: u64) -> [u8; 8] {
        index.to_be_bytes()
    }
}

impl RaftLogReader<NodeTypeConfig> for Storage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + std::fmt::Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry>, StorageError<NodeId>> {
        let cf = self
            .db
            .cf_handle("raft_log")
            .ok_or_else(|| StorageIOError::read(&std::io::Error::other("raft_log CF not found")))?;

        let start_bound = match range.start_bound() {
            std::ops::Bound::Included(&n) => n,
            std::ops::Bound::Excluded(&n) => n + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end_bound = match range.end_bound() {
            std::ops::Bound::Included(&n) => n + 1,
            std::ops::Bound::Excluded(&n) => n,
            std::ops::Bound::Unbounded => u64::MAX,
        };

        let mut entries = Vec::new();
        for index in start_bound..end_bound {
            let key = Self::log_index_to_key(index);
            if let Some(data) = self
                .db
                .get_cf(&cf, key)
                .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?
            {
                let entry = serde_json::from_slice(&data)
                    .map_err(|e| StorageIOError::read(&std::io::Error::other(e.to_string())))?;
                entries.push(entry);
            }
        }

        Ok(entries)
    }
}

impl RaftStorage<NodeTypeConfig> for Storage {
    type LogReader = Self;
    type SnapshotBuilder = SnapshotStorage;

    async fn save_vote(&mut self, vote: &Vote) -> Result<(), StorageError<NodeId>> {
        let cf = self.db.cf_handle("raft_state").ok_or_else(|| {
            StorageIOError::write(&std::io::Error::other("raft_state CF not found"))
        })?;

        let encoded = serde_json::to_vec(vote)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        self.db
            .put_cf(&cf, b"vote", encoded)
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote>, StorageError<NodeId>> {
        let cf = self.db.cf_handle("raft_state").ok_or_else(|| {
            StorageIOError::read(&std::io::Error::other("raft_state CF not found"))
        })?;

        let data = self
            .db
            .get_cf(&cf, b"vote")
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
        let last_purged_log_id = self.read_log_id(b"last_purged_log_id")?;
        let last_log_id = self.read_log_id(b"last_log_id")?;

        Ok(LogState {
            last_purged_log_id,
            last_log_id,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        Storage::new(Arc::clone(&self.db))
    }

    async fn append_to_log<I>(&mut self, entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry> + Send,
    {
        let cf = self.db.cf_handle("raft_log").ok_or_else(|| {
            StorageIOError::write(&std::io::Error::other("raft_log CF not found"))
        })?;

        let mut last_log_id = None;

        for entry in entries {
            let index = entry.log_id.index;
            let key = Self::log_index_to_key(index);

            let encoded = serde_json::to_vec(&entry)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

            self.db
                .put_cf(&cf, key, encoded)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;

            last_log_id = Some(entry.log_id);
        }

        if let Some(log_id) = last_log_id {
            self.write_log_id(b"last_log_id", &log_id)?;
        }

        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        log_id: LogId,
    ) -> Result<(), StorageError<NodeId>> {
        let start_index = log_id.index;
        let current_state = self.get_log_state().await?;

        if let Some(last_log_id) = current_state.last_log_id {
            let cf = self.db.cf_handle("raft_log").ok_or_else(|| {
                StorageIOError::write(&std::io::Error::other("raft_log CF not found"))
            })?;

            for index in start_index..=last_log_id.index {
                let key = Self::log_index_to_key(index);
                self.db
                    .delete_cf(&cf, key)
                    .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
            }

            if start_index > 0 {
                let new_last_log_id = LogId::new(log_id.leader_id, start_index - 1);
                self.write_log_id(b"last_log_id", &new_last_log_id)?;
            } else {
                let state_cf = self.db.cf_handle("raft_state").ok_or_else(|| {
                    StorageIOError::write(&std::io::Error::other("raft_state CF not found"))
                })?;
                self.db
                    .delete_cf(&state_cf, b"last_log_id")
                    .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
            }
        }

        Ok(())
    }

    async fn purge_logs_upto(&mut self, log_id: LogId) -> Result<(), StorageError<NodeId>> {
        let current_state = self.get_log_state().await?;
        let start_index = current_state
            .last_purged_log_id
            .map(|id| id.index + 1)
            .unwrap_or(0);

        let cf = self.db.cf_handle("raft_log").ok_or_else(|| {
            StorageIOError::write(&std::io::Error::other("raft_log CF not found"))
        })?;

        for index in start_index..=log_id.index {
            let key = Self::log_index_to_key(index);
            self.db
                .delete_cf(&cf, key)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }

        self.write_log_id(b"last_purged_log_id", &log_id)?;

        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId>, StoredMembership<NodeId, openraft::BasicNode>), StorageError<NodeId>>
    {
        let last_applied = self.read_log_id(b"last_applied_log_id")?;

        let cf = self.db.cf_handle("raft_state").ok_or_else(|| {
            StorageIOError::read(&std::io::Error::other("raft_state CF not found"))
        })?;

        let membership = match self
            .db
            .get_cf(&cf, b"membership")
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
        use super::commands::Command;

        let mut responses = Vec::new();
        let mut last_applied = None;

        for entry in entries {
            match &entry.payload {
                openraft::EntryPayload::Blank => {}
                openraft::EntryPayload::Normal(cmd) => match cmd {
                    Command::CreateBucket { bucket } => {
                        crate::bucket::create_bucket(&self.db, &bucket.name).map_err(|e| {
                            StorageIOError::write(&std::io::Error::other(e.to_string()))
                        })?;
                    }
                    Command::DeleteBucket { name } => {
                        crate::bucket::delete_bucket(&self.db, name).map_err(|e| {
                            StorageIOError::write(&std::io::Error::other(e.to_string()))
                        })?;
                    }
                    Command::PutObjectMetadata { metadata } => {
                        crate::object::put_object_metadata(&self.db, metadata).map_err(|e| {
                            StorageIOError::write(&std::io::Error::other(e.to_string()))
                        })?;
                    }
                    Command::DeleteObjectMetadata { bucket, key } => {
                        crate::object::delete_object_metadata(&self.db, bucket, key).map_err(
                            |e| StorageIOError::write(&std::io::Error::other(e.to_string())),
                        )?;
                    }
                },
                openraft::EntryPayload::Membership(membership) => {
                    let cf = self.db.cf_handle("raft_state").ok_or_else(|| {
                        StorageIOError::write(&std::io::Error::other("raft_state CF not found"))
                    })?;

                    let encoded = serde_json::to_vec(membership).map_err(|e| {
                        StorageIOError::write(&std::io::Error::other(e.to_string()))
                    })?;

                    self.db.put_cf(&cf, b"membership", encoded).map_err(|e| {
                        StorageIOError::write(&std::io::Error::other(e.to_string()))
                    })?;
                }
            }

            last_applied = Some(entry.log_id);
            responses.push(());
        }

        if let Some(log_id) = last_applied {
            self.write_log_id(b"last_applied_log_id", &log_id)?;
        }

        Ok(responses)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        SnapshotStorage::new(Arc::clone(&self.db))
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
        let db = Arc::clone(&self.db);
        let meta = meta.clone();
        tokio::task::spawn_blocking(move || super::snapshot::install_snapshot(&db, &meta, snapshot))
            .await
            .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<NodeTypeConfig>>, StorageError<NodeId>> {
        let db = Arc::clone(&self.db);
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
    use std::sync::Arc;
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
    async fn test_vote_persistence_empty_initially() {
        let (_temp_dir, mut storage) = create_test_storage();

        let vote = storage.read_vote().await.unwrap();
        assert_eq!(vote, None);
    }

    #[tokio::test]
    async fn test_vote_persistence_save_and_read() {
        let (_temp_dir, mut storage) = create_test_storage();

        let vote = Vote::new(5, 1);
        storage.save_vote(&vote).await.unwrap();

        let read_vote = storage.read_vote().await.unwrap();
        assert_eq!(read_vote, Some(vote));
    }

    #[tokio::test]
    async fn test_vote_persistence_overwrite() {
        let (_temp_dir, mut storage) = create_test_storage();

        let vote1 = Vote::new(5, 1);
        storage.save_vote(&vote1).await.unwrap();

        let vote2 = Vote::new(10, 2);
        storage.save_vote(&vote2).await.unwrap();

        let read_vote = storage.read_vote().await.unwrap();
        assert_eq!(read_vote, Some(vote2));
    }

    #[tokio::test]
    async fn test_log_state_empty_initially() {
        let (_temp_dir, mut storage) = create_test_storage();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, None);
        assert_eq!(state.last_purged_log_id, None);
    }

    #[tokio::test]
    async fn test_append_single_entry() {
        let (_temp_dir, mut storage) = create_test_storage();

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Blank,
        };

        storage.append_to_log(vec![entry.clone()]).await.unwrap();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, Some(log_id(1, 1, 1)));
    }

    #[tokio::test]
    async fn test_append_multiple_entries() {
        let (_temp_dir, mut storage) = create_test_storage();

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

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id, Some(log_id(1, 1, 3)));
    }

    #[tokio::test]
    async fn test_read_log_entries() {
        let (_temp_dir, mut storage) = create_test_storage();

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

        storage.append_to_log(entries.clone()).await.unwrap();

        let read_entries = storage.try_get_log_entries(1..4).await.unwrap();
        assert_eq!(read_entries.len(), 3);
        assert_eq!(read_entries[0].log_id, log_id(1, 1, 1));
        assert_eq!(read_entries[2].log_id, log_id(1, 1, 3));
    }

    #[tokio::test]
    async fn test_read_log_entries_partial_range() {
        let (_temp_dir, mut storage) = create_test_storage();

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

        let read_entries = storage.try_get_log_entries(2..3).await.unwrap();
        assert_eq!(read_entries.len(), 1);
        assert_eq!(read_entries[0].log_id, log_id(1, 1, 2));
    }

    #[tokio::test]
    async fn test_read_log_entries_empty_range() {
        let (_temp_dir, mut storage) = create_test_storage();

        let read_entries = storage.try_get_log_entries(1..1).await.unwrap();
        assert_eq!(read_entries.len(), 0);
    }

    #[tokio::test]
    async fn test_delete_conflict_logs_since() {
        let (_temp_dir, mut storage) = create_test_storage();

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

        let read_entries = storage.try_get_log_entries(1..4).await.unwrap();
        assert_eq!(read_entries.len(), 1);
    }

    #[tokio::test]
    async fn test_purge_logs_upto() {
        let (_temp_dir, mut storage) = create_test_storage();

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

        storage.purge_logs_upto(log_id(1, 1, 1)).await.unwrap();

        let state = storage.get_log_state().await.unwrap();
        assert_eq!(state.last_purged_log_id, Some(log_id(1, 1, 1)));

        let read_entries = storage.try_get_log_entries(1..4).await.unwrap();
        assert_eq!(read_entries.len(), 2);
        assert_eq!(read_entries[0].log_id, log_id(1, 1, 2));
    }

    #[tokio::test]
    async fn test_last_applied_state_empty_initially() {
        let (_temp_dir, mut storage) = create_test_storage();

        let (last_applied, membership) = storage.last_applied_state().await.unwrap();
        assert_eq!(last_applied, None);
        assert_eq!(membership, StoredMembership::default());
    }

    #[tokio::test]
    async fn test_apply_create_bucket_command() {
        let (_temp_dir, mut storage) = create_test_storage();

        let bucket = Bucket {
            name: "test-bucket".to_string(),
            created_at: chrono::Utc::now(),
        };

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Normal(crate::raft::Command::CreateBucket {
                bucket: bucket.clone(),
            }),
        };

        storage.apply_to_state_machine(&[entry]).await.unwrap();

        let (last_applied, _) = storage.last_applied_state().await.unwrap();
        assert_eq!(last_applied, Some(log_id(1, 1, 1)));

        let stored_bucket = crate::bucket::get_bucket(&storage.db, "test-bucket").unwrap();
        assert_eq!(stored_bucket.name, "test-bucket");
    }

    #[tokio::test]
    async fn test_apply_put_object_metadata_command() {
        let (_temp_dir, mut storage) = create_test_storage();

        let bucket = Bucket {
            name: "test-bucket".to_string(),
            created_at: chrono::Utc::now(),
        };
        crate::bucket::create_bucket(&storage.db, &bucket.name).unwrap();

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
            payload: EntryPayload::Normal(crate::raft::Command::PutObjectMetadata {
                metadata: metadata.clone(),
            }),
        };

        storage.apply_to_state_machine(&[entry]).await.unwrap();

        let stored_metadata =
            crate::object::get_object_metadata(&storage.db, "test-bucket", "test-key").unwrap();
        assert_eq!(stored_metadata.key, "test-key");
        assert_eq!(stored_metadata.size, 1024);
        assert_eq!(stored_metadata.replica_nodes, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_apply_multiple_commands() {
        let (_temp_dir, mut storage) = create_test_storage();

        let bucket1 = Bucket {
            name: "bucket1".to_string(),
            created_at: chrono::Utc::now(),
        };
        let bucket2 = Bucket {
            name: "bucket2".to_string(),
            created_at: chrono::Utc::now(),
        };

        let entries = vec![
            Entry {
                log_id: log_id(1, 1, 1),
                payload: EntryPayload::Normal(crate::raft::Command::CreateBucket {
                    bucket: bucket1,
                }),
            },
            Entry {
                log_id: log_id(1, 1, 2),
                payload: EntryPayload::Normal(crate::raft::Command::CreateBucket {
                    bucket: bucket2,
                }),
            },
        ];

        storage.apply_to_state_machine(&entries).await.unwrap();

        let (last_applied, _) = storage.last_applied_state().await.unwrap();
        assert_eq!(last_applied, Some(log_id(1, 1, 2)));

        assert!(crate::bucket::get_bucket(&storage.db, "bucket1").is_ok());
        assert!(crate::bucket::get_bucket(&storage.db, "bucket2").is_ok());
    }

    #[tokio::test]
    async fn test_apply_delete_bucket_command() {
        let (_temp_dir, mut storage) = create_test_storage();

        let bucket = Bucket {
            name: "test-bucket".to_string(),
            created_at: chrono::Utc::now(),
        };
        crate::bucket::create_bucket(&storage.db, &bucket.name).unwrap();

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Normal(crate::raft::Command::DeleteBucket {
                name: "test-bucket".to_string(),
            }),
        };

        storage.apply_to_state_machine(&[entry]).await.unwrap();

        assert!(crate::bucket::get_bucket(&storage.db, "test-bucket").is_err());
    }

    #[tokio::test]
    async fn test_apply_delete_object_metadata_command() {
        let (_temp_dir, mut storage) = create_test_storage();

        let bucket = Bucket {
            name: "test-bucket".to_string(),
            created_at: chrono::Utc::now(),
        };
        crate::bucket::create_bucket(&storage.db, &bucket.name).unwrap();

        let now = chrono::Utc::now();
        let metadata = ObjectMetadata {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            size: 1024,
            etag: "abc123".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: now,
            modified_at: now,
            replica_nodes: Vec::new(),
        };
        crate::object::put_object_metadata(&storage.db, &metadata).unwrap();

        let entry = Entry {
            log_id: log_id(1, 1, 1),
            payload: EntryPayload::Normal(crate::raft::Command::DeleteObjectMetadata {
                bucket: "test-bucket".to_string(),
                key: "test-key".to_string(),
            }),
        };

        storage.apply_to_state_machine(&[entry]).await.unwrap();

        assert!(
            crate::object::get_object_metadata(&storage.db, "test-bucket", "test-key").is_err()
        );
    }
}
