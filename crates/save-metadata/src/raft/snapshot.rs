use super::types::{NodeId, NodeTypeConfig};
use openraft::storage::RaftSnapshotBuilder;
use openraft::{SnapshotMeta, StorageError};
use std::io::Cursor;
use std::sync::Arc;

/// RocksDB checkpoint-based snapshot builder (stub).
pub struct SnapshotStorage {
    #[allow(dead_code)]
    db: Arc<rocksdb::DB>,
}

impl SnapshotStorage {
    pub fn new(db: Arc<rocksdb::DB>) -> Self {
        Self { db }
    }
}

impl RaftSnapshotBuilder<NodeTypeConfig> for SnapshotStorage {
    async fn build_snapshot(
        &mut self,
    ) -> Result<openraft::Snapshot<NodeTypeConfig>, StorageError<NodeId>> {
        // TODO: Create RocksDB checkpoint, compress to tar.gz, return with metadata
        let snapshot_data = Vec::new();

        let snapshot = openraft::Snapshot {
            meta: SnapshotMeta::default(),
            snapshot: Box::new(Cursor::new(snapshot_data)),
        };

        Ok(snapshot)
    }
}
