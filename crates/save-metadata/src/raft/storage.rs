use super::snapshot::SnapshotStorage;
use super::types::{Entry, LogId, NodeId, NodeTypeConfig, Vote};
use openraft::{
    LogState, RaftLogReader, RaftStorage, Snapshot, SnapshotMeta, StorageError, StoredMembership,
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
}

impl RaftLogReader<NodeTypeConfig> for Storage {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + std::fmt::Debug + Send>(
        &mut self,
        _range: RB,
    ) -> Result<Vec<Entry>, StorageError<NodeId>> {
        // TODO: Implement reading from raft_log column family
        Ok(Vec::new())
    }
}

impl RaftStorage<NodeTypeConfig> for Storage {
    type LogReader = Self;
    type SnapshotBuilder = SnapshotStorage;

    async fn save_vote(&mut self, _vote: &Vote) -> Result<(), StorageError<NodeId>> {
        // TODO: Implement saving vote to raft_state CF
        Ok(())
    }

    async fn read_vote(&mut self) -> Result<Option<Vote>, StorageError<NodeId>> {
        // TODO: Implement reading vote from raft_state CF
        Ok(None)
    }

    async fn get_log_state(&mut self) -> Result<LogState<NodeTypeConfig>, StorageError<NodeId>> {
        // TODO: Implement reading last log id from raft_log CF
        Ok(LogState {
            last_purged_log_id: None,
            last_log_id: None,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        Storage::new(Arc::clone(&self.db))
    }

    async fn append_to_log<I>(&mut self, _entries: I) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry> + Send,
    {
        // TODO: Implement appending entries to raft_log CF
        Ok(())
    }

    async fn delete_conflict_logs_since(
        &mut self,
        _log_id: LogId,
    ) -> Result<(), StorageError<NodeId>> {
        // TODO: Implement truncating log entries from raft_log CF
        Ok(())
    }

    async fn purge_logs_upto(&mut self, _log_id: LogId) -> Result<(), StorageError<NodeId>> {
        // TODO: Implement purging log entries from raft_log CF
        Ok(())
    }

    async fn last_applied_state(
        &mut self,
    ) -> Result<(Option<LogId>, StoredMembership<NodeId, openraft::BasicNode>), StorageError<NodeId>>
    {
        // TODO: Implement reading last applied state
        Ok((None, StoredMembership::default()))
    }

    async fn apply_to_state_machine(
        &mut self,
        entries: &[Entry],
    ) -> Result<Vec<()>, StorageError<NodeId>> {
        let mut responses = Vec::new();

        for _entry in entries {
            // TODO: Deserialize Command from entry.payload, execute bucket::* or object::* operation
            responses.push(());
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
        _meta: &SnapshotMeta<NodeId, openraft::BasicNode>,
        _snapshot: Box<std::io::Cursor<Vec<u8>>>,
    ) -> Result<(), StorageError<NodeId>> {
        // TODO: Restore RocksDB from snapshot
        Ok(())
    }

    async fn get_current_snapshot(
        &mut self,
    ) -> Result<Option<Snapshot<NodeTypeConfig>>, StorageError<NodeId>> {
        // TODO: Return current snapshot if available
        Ok(None)
    }
}
