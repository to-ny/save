use crate::error::{MetadataError, Result};
use chrono::{DateTime, Utc};
use save_common::validate_object_key;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectMetadata {
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub content_type: Option<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    /// Node IDs that hold replicas of this object (including local node).
    #[serde(default)]
    pub replica_nodes: Vec<u64>,
}

impl ObjectMetadata {
    pub fn new(bucket: String, key: String, size: u64, etag: String) -> Self {
        let now = Utc::now();
        Self {
            bucket,
            key,
            size,
            etag,
            content_type: None,
            created_at: now,
            modified_at: now,
            replica_nodes: Vec::new(),
        }
    }

    pub fn with_replicas(mut self, nodes: Vec<u64>) -> Self {
        self.replica_nodes = nodes;
        self
    }

    pub(crate) fn db_key(bucket: &str, key: &str) -> String {
        format!("obj:{}/{}", bucket, key)
    }
}

fn prepare_metadata_write(metadata: &ObjectMetadata) -> Result<(Vec<u8>, Vec<u8>)> {
    validate_object_key(&metadata.key)
        .map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;

    let key = ObjectMetadata::db_key(&metadata.bucket, &metadata.key);
    let value = bincode::serde::encode_to_vec(metadata, bincode::config::standard())?;
    Ok((key.into_bytes(), value))
}

pub(crate) fn put_object_metadata(db: &rocksdb::DB, metadata: &ObjectMetadata) -> Result<()> {
    let (key, value) = prepare_metadata_write(metadata)?;
    db.put(&key, value)?;
    Ok(())
}

/// Atomically commits object metadata using WriteBatch with sync=true.
pub(crate) fn commit_object_metadata(db: &rocksdb::DB, metadata: &ObjectMetadata) -> Result<()> {
    let (key, value) = prepare_metadata_write(metadata)?;

    let mut batch = rocksdb::WriteBatch::default();
    batch.put(&key, value);

    let mut write_opts = rocksdb::WriteOptions::default();
    write_opts.set_sync(true);

    db.write_opt(batch, &write_opts)?;

    Ok(())
}

pub(crate) fn get_object_metadata(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
) -> Result<ObjectMetadata> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;

    let db_key = ObjectMetadata::db_key(bucket, key);

    match db.get(&db_key)? {
        Some(data) => {
            let (metadata, _) =
                bincode::serde::decode_from_slice(&data, bincode::config::standard())?;
            Ok(metadata)
        }
        None => Err(MetadataError::ObjectNotFound {
            bucket: bucket.to_string(),
            key: key.to_string(),
        }),
    }
}

pub(crate) fn delete_object_metadata(db: &rocksdb::DB, bucket: &str, key: &str) -> Result<()> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;

    let db_key = ObjectMetadata::db_key(bucket, key);

    db.delete(&db_key)?;

    #[cfg(feature = "failpoints")]
    fail::fail_point!("metadata_delete_after_write");

    Ok(())
}

pub(crate) fn list_objects(
    db: &rocksdb::DB,
    bucket: &str,
    prefix: Option<&str>,
) -> Result<Vec<ObjectMetadata>> {
    let key_prefix = match prefix {
        Some(p) => format!("obj:{}/{}", bucket, p),
        None => format!("obj:{}/", bucket),
    };

    let mut objects = Vec::new();
    let iter = db.prefix_iterator(&key_prefix);

    for item in iter {
        let (key, value) = item?;
        if !key.starts_with(key_prefix.as_bytes()) {
            break;
        }
        let (metadata, _): (ObjectMetadata, _) =
            bincode::serde::decode_from_slice(&value, bincode::config::standard())?;
        objects.push(metadata);
    }

    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> rocksdb::DB {
        let path = tempfile::tempdir().unwrap();
        rocksdb::DB::open_default(path.path()).unwrap()
    }

    #[test]
    fn test_put_and_get_object_metadata() {
        let db = create_test_db();
        let metadata = ObjectMetadata::new(
            "test-bucket".to_string(),
            "test-key".to_string(),
            1024,
            "abc123".to_string(),
        );

        put_object_metadata(&db, &metadata).unwrap();
        let fetched = get_object_metadata(&db, "test-bucket", "test-key").unwrap();

        assert_eq!(metadata, fetched);
    }

    #[test]
    fn test_get_object_metadata_not_found() {
        let db = create_test_db();
        let result = get_object_metadata(&db, "test-bucket", "nonexistent");
        assert!(matches!(result, Err(MetadataError::ObjectNotFound { .. })));
    }

    #[test]
    fn test_delete_object_metadata() {
        let db = create_test_db();
        let metadata = ObjectMetadata::new(
            "test-bucket".to_string(),
            "test-key".to_string(),
            1024,
            "abc123".to_string(),
        );

        put_object_metadata(&db, &metadata).unwrap();
        delete_object_metadata(&db, "test-bucket", "test-key").unwrap();

        let result = get_object_metadata(&db, "test-bucket", "test-key");
        assert!(matches!(result, Err(MetadataError::ObjectNotFound { .. })));
    }

    #[test]
    fn test_list_objects() {
        let db = create_test_db();

        let obj1 = ObjectMetadata::new(
            "bucket".to_string(),
            "file1.txt".to_string(),
            100,
            "etag1".to_string(),
        );
        let obj2 = ObjectMetadata::new(
            "bucket".to_string(),
            "file2.txt".to_string(),
            200,
            "etag2".to_string(),
        );

        put_object_metadata(&db, &obj1).unwrap();
        put_object_metadata(&db, &obj2).unwrap();

        let objects = list_objects(&db, "bucket", None).unwrap();
        assert_eq!(objects.len(), 2);
    }

    #[test]
    fn test_list_objects_with_prefix() {
        let db = create_test_db();

        let obj1 = ObjectMetadata::new(
            "bucket".to_string(),
            "docs/file1.txt".to_string(),
            100,
            "etag1".to_string(),
        );
        let obj2 = ObjectMetadata::new(
            "bucket".to_string(),
            "docs/file2.txt".to_string(),
            200,
            "etag2".to_string(),
        );
        let obj3 = ObjectMetadata::new(
            "bucket".to_string(),
            "images/file3.jpg".to_string(),
            300,
            "etag3".to_string(),
        );

        put_object_metadata(&db, &obj1).unwrap();
        put_object_metadata(&db, &obj2).unwrap();
        put_object_metadata(&db, &obj3).unwrap();

        let objects = list_objects(&db, "bucket", Some("docs/")).unwrap();
        assert_eq!(objects.len(), 2);
        assert!(objects.iter().all(|o| o.key.starts_with("docs/")));
    }

    #[test]
    fn test_validate_key_valid() {
        assert!(validate_object_key("file.txt").is_ok());
        assert!(validate_object_key("path/to/file.txt").is_ok());
        assert!(validate_object_key("a").is_ok());
        assert!(validate_object_key(&"x".repeat(1024)).is_ok());
    }

    #[test]
    fn test_validate_key_empty() {
        assert!(validate_object_key("").is_err());
    }

    #[test]
    fn test_validate_key_too_long() {
        let long_key = "x".repeat(1025);
        assert!(validate_object_key(&long_key).is_err());
    }

    #[test]
    fn test_validate_key_null_byte() {
        assert!(validate_object_key("file\0.txt").is_err());
    }

    #[test]
    fn test_commit_object_metadata_with_writebatch() {
        let db = create_test_db();
        let metadata = ObjectMetadata::new(
            "test-bucket".to_string(),
            "test-key".to_string(),
            2048,
            "xyz789".to_string(),
        );

        commit_object_metadata(&db, &metadata).unwrap();
        let fetched = get_object_metadata(&db, "test-bucket", "test-key").unwrap();

        assert_eq!(metadata, fetched);
    }

    #[test]
    fn test_commit_object_metadata_durability() {
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_db");

        {
            let db = rocksdb::DB::open_default(&db_path).unwrap();
            let metadata = ObjectMetadata::new(
                "bucket".to_string(),
                "durable-object".to_string(),
                4096,
                "abcdef".to_string(),
            );

            commit_object_metadata(&db, &metadata).unwrap();
        }

        let db = rocksdb::DB::open_default(&db_path).unwrap();
        let fetched = get_object_metadata(&db, "bucket", "durable-object").unwrap();

        assert_eq!(fetched.size, 4096);
        assert_eq!(fetched.etag, "abcdef");
    }

    #[test]
    fn test_new_has_empty_replica_nodes() {
        let metadata = ObjectMetadata::new(
            "bucket".to_string(),
            "key".to_string(),
            100,
            "etag".to_string(),
        );
        assert!(metadata.replica_nodes.is_empty());
    }

    #[test]
    fn test_with_replicas() {
        let metadata = ObjectMetadata::new(
            "bucket".to_string(),
            "key".to_string(),
            100,
            "etag".to_string(),
        )
        .with_replicas(vec![1, 2, 3]);

        assert_eq!(metadata.replica_nodes, vec![1, 2, 3]);
    }

    #[test]
    fn test_replica_nodes_serialization() {
        let db = create_test_db();
        let metadata = ObjectMetadata::new(
            "bucket".to_string(),
            "key".to_string(),
            100,
            "etag".to_string(),
        )
        .with_replicas(vec![10, 20, 30]);

        put_object_metadata(&db, &metadata).unwrap();
        let fetched = get_object_metadata(&db, "bucket", "key").unwrap();

        assert_eq!(fetched.replica_nodes, vec![10, 20, 30]);
    }
}
