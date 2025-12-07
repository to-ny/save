//! State machine command application logic.

use super::commands::Command;
use openraft::StorageIOError;
use std::sync::Arc;

type NodeId = u64;

/// Apply a command to the state machine.
pub fn apply_command(db: &Arc<rocksdb::DB>, cmd: &Command) -> Result<(), StorageIOError<NodeId>> {
    match cmd {
        Command::CreateBucket { bucket } => {
            crate::bucket::create_bucket(db, &bucket.name)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }
        Command::DeleteBucket { name } => {
            crate::bucket::delete_bucket(db, name)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }
        Command::PutObjectMetadata { metadata } => {
            crate::object::put_object_metadata(db, metadata)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }
        Command::DeleteObjectMetadata { bucket, key } => {
            crate::object::delete_object_metadata(db, bucket, key)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }
        Command::AcquireLock {
            bucket,
            key,
            lock_type,
            holder,
        } => {
            let _ = crate::lock::acquire_lock(db, bucket, key, *lock_type, holder)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }
        Command::ReleaseLock {
            bucket,
            key,
            holder,
        } => {
            crate::lock::release_lock(db, bucket, key, holder)
                .map_err(|e| StorageIOError::write(&std::io::Error::other(e.to_string())))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bucket;
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
    fn test_apply_create_bucket() {
        let (_temp, db) = create_test_db();

        let bucket = Bucket {
            name: "test-bucket".to_string(),
            created_at: chrono::Utc::now(),
        };

        apply_command(&db, &Command::CreateBucket { bucket }).unwrap();

        let stored = crate::bucket::get_bucket(&db, "test-bucket").unwrap();
        assert_eq!(stored.name, "test-bucket");
    }

    #[test]
    fn test_apply_delete_bucket() {
        let (_temp, db) = create_test_db();

        crate::bucket::create_bucket(&db, "test-bucket").unwrap();

        apply_command(
            &db,
            &Command::DeleteBucket {
                name: "test-bucket".to_string(),
            },
        )
        .unwrap();

        assert!(crate::bucket::get_bucket(&db, "test-bucket").is_err());
    }

    #[test]
    fn test_apply_put_object_metadata() {
        let (_temp, db) = create_test_db();

        crate::bucket::create_bucket(&db, "test-bucket").unwrap();

        let now = chrono::Utc::now();
        let metadata = crate::ObjectMetadata {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            size: 1024,
            etag: "abc123".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: now,
            modified_at: now,
            replica_nodes: vec![1, 2, 3],
        };

        apply_command(&db, &Command::PutObjectMetadata { metadata }).unwrap();

        let stored = crate::object::get_object_metadata(&db, "test-bucket", "test-key").unwrap();
        assert_eq!(stored.key, "test-key");
        assert_eq!(stored.replica_nodes, vec![1, 2, 3]);
    }

    #[test]
    fn test_apply_delete_object_metadata() {
        let (_temp, db) = create_test_db();

        crate::bucket::create_bucket(&db, "test-bucket").unwrap();

        let now = chrono::Utc::now();
        let metadata = crate::ObjectMetadata {
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            size: 1024,
            etag: "abc123".to_string(),
            content_type: None,
            created_at: now,
            modified_at: now,
            replica_nodes: Vec::new(),
        };
        crate::object::put_object_metadata(&db, &metadata).unwrap();

        apply_command(
            &db,
            &Command::DeleteObjectMetadata {
                bucket: "test-bucket".to_string(),
                key: "test-key".to_string(),
            },
        )
        .unwrap();

        assert!(crate::object::get_object_metadata(&db, "test-bucket", "test-key").is_err());
    }
}
