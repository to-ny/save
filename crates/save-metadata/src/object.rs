use crate::error::{MetadataError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(MetadataError::InvalidOperation(
            "Object key cannot be empty".to_string()
        ));
    }

    if key.len() > 1024 {
        return Err(MetadataError::InvalidOperation(
            format!("Object key too long: {} bytes (max 1024)", key.len())
        ));
    }

    if key.contains('\0') {
        return Err(MetadataError::InvalidOperation(
            "Object key cannot contain null bytes".to_string()
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObjectMetadata {
    pub bucket: String,
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub content_type: Option<String>,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
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
        }
    }

    pub(crate) fn db_key(bucket: &str, key: &str) -> String {
        format!("obj:{}/{}", bucket, key)
    }
}

pub(crate) fn put_object_metadata(
    db: &rocksdb::DB,
    metadata: &ObjectMetadata,
) -> Result<()> {
    validate_key(&metadata.key)?;

    let key = ObjectMetadata::db_key(&metadata.bucket, &metadata.key);
    let value = bincode::serialize(metadata)?;
    db.put(&key, value)?;
    Ok(())
}

pub(crate) fn get_object_metadata(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
) -> Result<ObjectMetadata> {
    validate_key(key)?;

    let db_key = ObjectMetadata::db_key(bucket, key);

    match db.get(&db_key)? {
        Some(data) => {
            let metadata = bincode::deserialize(&data)?;
            Ok(metadata)
        }
        None => Err(MetadataError::ObjectNotFound {
            bucket: bucket.to_string(),
            key: key.to_string(),
        }),
    }
}

pub(crate) fn delete_object_metadata(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
) -> Result<()> {
    validate_key(key)?;

    let db_key = ObjectMetadata::db_key(bucket, key);
    db.delete(&db_key)?;
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
        let metadata: ObjectMetadata = bincode::deserialize(&value)?;
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
        assert!(validate_key("file.txt").is_ok());
        assert!(validate_key("path/to/file.txt").is_ok());
        assert!(validate_key("a").is_ok());
        assert!(validate_key(&"x".repeat(1024)).is_ok());
    }

    #[test]
    fn test_validate_key_empty() {
        let result = validate_key("");
        assert!(matches!(result, Err(MetadataError::InvalidOperation(_))));
    }

    #[test]
    fn test_validate_key_too_long() {
        let long_key = "x".repeat(1025);
        let result = validate_key(&long_key);
        assert!(matches!(result, Err(MetadataError::InvalidOperation(_))));
    }

    #[test]
    fn test_validate_key_null_byte() {
        let result = validate_key("file\0.txt");
        assert!(matches!(result, Err(MetadataError::InvalidOperation(_))));
    }
}
