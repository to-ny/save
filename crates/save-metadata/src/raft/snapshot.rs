//! Raft snapshot implementation using RocksDB checkpoints.

use super::types::{NodeId, NodeTypeConfig};
use crate::keys::DATA_PREFIXES;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use openraft::storage::RaftSnapshotBuilder;
use openraft::{Snapshot, SnapshotMeta, StorageError, StorageIOError, StoredMembership};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use tar::{Archive, Builder};
use tempfile::TempDir;
use tracing::{debug, info};

const SNAPSHOT_META_KEY: &[u8] = b"snapshot_meta";
const SNAPSHOT_DATA_KEY: &[u8] = b"snapshot_data";

type AppliedState = (
    Option<openraft::LogId<NodeId>>,
    StoredMembership<NodeId, openraft::BasicNode>,
);

fn read_err<E: ToString>(e: E) -> StorageError<NodeId> {
    StorageIOError::read(&std::io::Error::other(e.to_string())).into()
}

fn write_err<E: ToString>(e: E) -> StorageError<NodeId> {
    StorageIOError::write(&std::io::Error::other(e.to_string())).into()
}

/// RocksDB checkpoint-based snapshot builder.
pub struct SnapshotStorage {
    db: Arc<rocksdb::DB>,
}

impl SnapshotStorage {
    pub fn new(db: Arc<rocksdb::DB>) -> Self {
        Self { db }
    }

    #[allow(clippy::result_large_err)]
    fn cf(&self) -> Result<&rocksdb::ColumnFamily, StorageError<NodeId>> {
        self.db
            .cf_handle("raft_snapshot")
            .ok_or_else(|| read_err("raft_snapshot CF not found"))
    }
}

#[allow(clippy::result_large_err)]
fn create_checkpoint_archive(db: &rocksdb::DB) -> Result<Vec<u8>, StorageError<NodeId>> {
    let temp_dir = TempDir::new().map_err(write_err)?;
    let checkpoint_path = temp_dir.path().join("checkpoint");

    let checkpoint = rocksdb::checkpoint::Checkpoint::new(db).map_err(write_err)?;
    checkpoint
        .create_checkpoint(&checkpoint_path)
        .map_err(write_err)?;

    let mut archive_data = Vec::new();
    {
        let encoder = GzEncoder::new(&mut archive_data, Compression::default());
        let mut tar = Builder::new(encoder);
        tar.append_dir_all(".", &checkpoint_path)
            .map_err(write_err)?;
        tar.into_inner()
            .map_err(write_err)?
            .finish()
            .map_err(write_err)?;
    }

    Ok(archive_data)
}

impl RaftSnapshotBuilder<NodeTypeConfig> for SnapshotStorage {
    async fn build_snapshot(&mut self) -> Result<Snapshot<NodeTypeConfig>, StorageError<NodeId>> {
        debug!("building raft snapshot");

        let db = Arc::clone(&self.db);
        let (last_applied, membership, snapshot_data) =
            tokio::task::spawn_blocking(move || -> Result<_, StorageError<NodeId>> {
                let (last_applied, membership) = read_applied_state(&db)?;
                let snapshot_data = create_checkpoint_archive(&db)?;
                Ok((last_applied, membership, snapshot_data))
            })
            .await
            .map_err(|e| write_err(e.to_string()))??;

        let snapshot_id = format!(
            "{}-{}",
            last_applied.map(|id| id.index).unwrap_or(0),
            chrono::Utc::now().timestamp_millis()
        );

        let meta = SnapshotMeta {
            last_log_id: last_applied,
            last_membership: membership,
            snapshot_id,
        };

        info!(
            snapshot_id = %meta.snapshot_id,
            last_log_index = ?meta.last_log_id.map(|id| id.index),
            size_bytes = snapshot_data.len(),
            "snapshot created"
        );

        let cf = self.cf()?;

        let meta_bytes = serde_json::to_vec(&meta).map_err(write_err)?;
        self.db
            .put_cf(cf, SNAPSHOT_META_KEY, &meta_bytes)
            .map_err(write_err)?;

        self.db
            .put_cf(cf, SNAPSHOT_DATA_KEY, &snapshot_data)
            .map_err(write_err)?;

        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(snapshot_data)),
        })
    }
}

#[allow(clippy::result_large_err)]
fn read_applied_state(db: &rocksdb::DB) -> Result<AppliedState, StorageError<NodeId>> {
    let cf = db
        .cf_handle("raft_state")
        .ok_or_else(|| read_err("raft_state CF not found"))?;

    let last_applied = match db.get_cf(cf, b"last_applied_log_id").map_err(read_err)? {
        Some(bytes) => Some(serde_json::from_slice(&bytes).map_err(read_err)?),
        None => None,
    };

    let membership = match db.get_cf(cf, b"membership").map_err(read_err)? {
        Some(data) => serde_json::from_slice(&data).map_err(read_err)?,
        None => StoredMembership::default(),
    };

    Ok((last_applied, membership))
}

#[allow(clippy::result_large_err)]
pub fn get_current_snapshot(
    db: &rocksdb::DB,
) -> Result<Option<Snapshot<NodeTypeConfig>>, StorageError<NodeId>> {
    let cf = db
        .cf_handle("raft_snapshot")
        .ok_or_else(|| read_err("raft_snapshot CF not found"))?;

    let meta_bytes = match db.get_cf(cf, SNAPSHOT_META_KEY).map_err(read_err)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };

    let data = match db.get_cf(cf, SNAPSHOT_DATA_KEY).map_err(read_err)? {
        Some(bytes) => bytes,
        None => return Ok(None),
    };

    let meta: SnapshotMeta<NodeId, openraft::BasicNode> =
        serde_json::from_slice(&meta_bytes).map_err(read_err)?;

    Ok(Some(Snapshot {
        meta,
        snapshot: Box::new(Cursor::new(data)),
    }))
}

/// Installs a snapshot by extracting and restoring the RocksDB checkpoint.
#[allow(clippy::result_large_err, clippy::boxed_local)]
pub fn install_snapshot(
    db: &rocksdb::DB,
    meta: &SnapshotMeta<NodeId, openraft::BasicNode>,
    snapshot: Box<Cursor<Vec<u8>>>,
) -> Result<(), StorageError<NodeId>> {
    info!(
        snapshot_id = %meta.snapshot_id,
        last_log_index = ?meta.last_log_id.map(|id| id.index),
        "installing snapshot"
    );

    let temp_dir = TempDir::new().map_err(write_err)?;

    let snapshot_data = (*snapshot).into_inner();
    debug!(size_bytes = snapshot_data.len(), "decompressing snapshot");

    let decoder = GzDecoder::new(&snapshot_data[..]);
    let mut archive = Archive::new(decoder);

    archive.unpack(temp_dir.path()).map_err(write_err)?;

    let extracted_db = open_checkpoint_readonly(temp_dir.path())?;

    restore_default_cf(db, &extracted_db)?;
    update_raft_state_from_snapshot(db, meta)?;

    let cf = db
        .cf_handle("raft_snapshot")
        .ok_or_else(|| write_err("raft_snapshot CF not found"))?;

    let meta_bytes = serde_json::to_vec(meta).map_err(write_err)?;
    db.put_cf(cf, SNAPSHOT_META_KEY, &meta_bytes)
        .map_err(write_err)?;
    db.put_cf(cf, SNAPSHOT_DATA_KEY, snapshot_data)
        .map_err(write_err)?;

    info!(snapshot_id = %meta.snapshot_id, "snapshot installed");
    Ok(())
}

#[allow(clippy::result_large_err)]
fn open_checkpoint_readonly(path: &Path) -> Result<rocksdb::DB, StorageError<NodeId>> {
    let mut opts = rocksdb::Options::default();
    opts.create_if_missing(false);
    rocksdb::DB::open_for_read_only(&opts, path, false).map_err(read_err)
}

#[allow(clippy::result_large_err)]
fn restore_default_cf(
    target_db: &rocksdb::DB,
    source_db: &rocksdb::DB,
) -> Result<(), StorageError<NodeId>> {
    for prefix in DATA_PREFIXES {
        let iter = target_db.prefix_iterator(prefix);
        for item in iter {
            let (key, _) = item.map_err(write_err)?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            target_db.delete(&key).map_err(write_err)?;
        }
    }

    for prefix in DATA_PREFIXES {
        let iter = source_db.prefix_iterator(prefix);
        for item in iter {
            let (key, value) = item.map_err(read_err)?;
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            target_db.put(&key, &value).map_err(write_err)?;
        }
    }

    Ok(())
}

#[allow(clippy::result_large_err)]
fn update_raft_state_from_snapshot(
    db: &rocksdb::DB,
    meta: &SnapshotMeta<NodeId, openraft::BasicNode>,
) -> Result<(), StorageError<NodeId>> {
    let cf = db
        .cf_handle("raft_state")
        .ok_or_else(|| write_err("raft_state CF not found"))?;

    if let Some(last_log_id) = &meta.last_log_id {
        let encoded = serde_json::to_vec(last_log_id).map_err(write_err)?;
        db.put_cf(cf, b"last_applied_log_id", encoded)
            .map_err(write_err)?;
    }

    let membership_bytes = serde_json::to_vec(&meta.last_membership).map_err(write_err)?;
    db.put_cf(cf, b"membership", membership_bytes)
        .map_err(write_err)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raft::types::LogId;
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

    #[tokio::test]
    async fn test_build_snapshot_empty_db() {
        let (_temp_dir, db) = create_test_db();
        let mut storage = SnapshotStorage::new(db);

        let snapshot = storage.build_snapshot().await.unwrap();
        assert!(snapshot.meta.last_log_id.is_none());
        assert!(!snapshot.meta.snapshot_id.is_empty());
    }

    #[tokio::test]
    async fn test_build_snapshot_with_data() {
        let (_temp_dir, db) = create_test_db();

        db.put("bkt:test-bucket", b"bucket-data").unwrap();
        db.put("obj:test-bucket/key1", b"object-data").unwrap();

        let cf = db.cf_handle("raft_state").unwrap();
        let log_id = log_id(1, 1, 5);
        db.put_cf(
            cf,
            b"last_applied_log_id",
            serde_json::to_vec(&log_id).unwrap(),
        )
        .unwrap();

        let mut storage = SnapshotStorage::new(db.clone());
        let snapshot = storage.build_snapshot().await.unwrap();

        assert_eq!(snapshot.meta.last_log_id, Some(log_id));
        assert!(!snapshot.snapshot.get_ref().is_empty());
    }

    #[tokio::test]
    async fn test_get_current_snapshot_none() {
        let (_temp_dir, db) = create_test_db();
        let result = get_current_snapshot(&db).unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_snapshot_roundtrip() {
        let (_temp_dir, db) = create_test_db();

        db.put("bkt:my-bucket", b"bucket-data").unwrap();
        db.put("obj:my-bucket/file.txt", b"object-data").unwrap();

        let mut storage = SnapshotStorage::new(db.clone());
        let snapshot = storage.build_snapshot().await.unwrap();
        let snapshot_id = snapshot.meta.snapshot_id.clone();

        let retrieved = get_current_snapshot(&db).unwrap().unwrap();
        assert_eq!(retrieved.meta.snapshot_id, snapshot_id);
    }

    #[tokio::test]
    async fn test_install_snapshot() {
        let (_temp_dir1, source_db) = create_test_db();
        source_db.put("bkt:source-bucket", b"source-data").unwrap();
        source_db
            .put("obj:source-bucket/key", b"source-obj")
            .unwrap();

        let cf = source_db.cf_handle("raft_state").unwrap();
        let log_id = log_id(1, 1, 10);
        source_db
            .put_cf(
                cf,
                b"last_applied_log_id",
                serde_json::to_vec(&log_id).unwrap(),
            )
            .unwrap();

        let mut storage = SnapshotStorage::new(source_db.clone());
        let snapshot = storage.build_snapshot().await.unwrap();

        let (_temp_dir2, target_db) = create_test_db();
        target_db.put("bkt:target-bucket", b"target-data").unwrap();

        let snapshot_data = snapshot.snapshot.get_ref().clone();
        install_snapshot(
            &target_db,
            &snapshot.meta,
            Box::new(Cursor::new(snapshot_data)),
        )
        .unwrap();

        assert!(target_db.get("bkt:source-bucket").unwrap().is_some());
        assert!(target_db.get("obj:source-bucket/key").unwrap().is_some());
        assert!(target_db.get("bkt:target-bucket").unwrap().is_none());

        let cf = target_db.cf_handle("raft_state").unwrap();
        let applied: LogId = serde_json::from_slice(
            &target_db
                .get_cf(cf, b"last_applied_log_id")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(applied.index, 10);
    }
}
