use crate::error::{MetadataError, Result};
use save_common::{Bucket, validate_bucket_name};

fn bucket_key(name: &str) -> String {
    format!("bkt:{}", name)
}

pub(crate) fn create_bucket(db: &rocksdb::DB, name: &str) -> Result<Bucket> {
    validate_bucket_name(name).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;

    let key = bucket_key(name);

    if db.get(&key)?.is_some() {
        return Err(MetadataError::BucketAlreadyExists(name.to_string()));
    }

    let bucket = Bucket::new(name.to_string());
    let value = bincode::serialize(&bucket)?;
    db.put(&key, value)?;

    Ok(bucket)
}

pub(crate) fn get_bucket(db: &rocksdb::DB, name: &str) -> Result<Bucket> {
    validate_bucket_name(name).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;

    let key = bucket_key(name);

    match db.get(&key)? {
        Some(data) => {
            let bucket = bincode::deserialize(&data)?;
            Ok(bucket)
        }
        None => Err(MetadataError::BucketNotFound(name.to_string())),
    }
}

pub(crate) fn delete_bucket(db: &rocksdb::DB, name: &str) -> Result<()> {
    validate_bucket_name(name).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;

    let key = bucket_key(name);
    db.delete(&key)?;
    Ok(())
}

pub(crate) fn list_buckets(db: &rocksdb::DB) -> Result<Vec<Bucket>> {
    let prefix = "bkt:";
    let mut buckets = Vec::new();

    let iter = db.prefix_iterator(prefix);
    for item in iter {
        let (key, value) = item?;
        if !key.starts_with(prefix.as_bytes()) {
            break;
        }
        let bucket: Bucket = bincode::deserialize(&value)?;
        buckets.push(bucket);
    }

    Ok(buckets)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> rocksdb::DB {
        let path = tempfile::tempdir().unwrap();
        rocksdb::DB::open_default(path.path()).unwrap()
    }

    #[test]
    fn test_create_bucket() {
        let db = create_test_db();
        let bucket = create_bucket(&db, "test-bucket").unwrap();
        assert_eq!(bucket.name, "test-bucket");
    }

    #[test]
    fn test_create_bucket_duplicate() {
        let db = create_test_db();
        create_bucket(&db, "test-bucket").unwrap();
        let result = create_bucket(&db, "test-bucket");
        assert!(matches!(result, Err(MetadataError::BucketAlreadyExists(_))));
    }

    #[test]
    fn test_get_bucket() {
        let db = create_test_db();
        let created = create_bucket(&db, "test-bucket").unwrap();
        let fetched = get_bucket(&db, "test-bucket").unwrap();
        assert_eq!(created, fetched);
    }

    #[test]
    fn test_get_bucket_not_found() {
        let db = create_test_db();
        let result = get_bucket(&db, "nonexistent");
        assert!(matches!(result, Err(MetadataError::BucketNotFound(_))));
    }

    #[test]
    fn test_delete_bucket() {
        let db = create_test_db();
        create_bucket(&db, "test-bucket").unwrap();
        delete_bucket(&db, "test-bucket").unwrap();
        let result = get_bucket(&db, "test-bucket");
        assert!(matches!(result, Err(MetadataError::BucketNotFound(_))));
    }

    #[test]
    fn test_list_buckets() {
        let db = create_test_db();
        create_bucket(&db, "bucket1").unwrap();
        create_bucket(&db, "bucket2").unwrap();
        create_bucket(&db, "bucket3").unwrap();

        let buckets = list_buckets(&db).unwrap();
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].name, "bucket1");
        assert_eq!(buckets[1].name, "bucket2");
        assert_eq!(buckets[2].name, "bucket3");
    }

    #[test]
    fn test_validate_bucket_name_valid() {
        assert!(validate_bucket_name("my-bucket").is_ok());
        assert!(validate_bucket_name("my.bucket").is_ok());
        assert!(validate_bucket_name("bucket123").is_ok());
        assert!(validate_bucket_name("abc").is_ok());
        assert!(validate_bucket_name(&format!("a{}", "b".repeat(62))).is_ok());
    }

    #[test]
    fn test_validate_bucket_name_empty() {
        assert!(validate_bucket_name("").is_err());
    }

    #[test]
    fn test_validate_bucket_name_too_long() {
        let long_name = "a".repeat(64);
        assert!(validate_bucket_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_bucket_name_invalid_characters() {
        assert!(validate_bucket_name("MyBucket").is_err());
        assert!(validate_bucket_name("bucket_name").is_err());
        assert!(validate_bucket_name("bucket name").is_err());
        assert!(validate_bucket_name("bucket@example").is_err());
    }

    #[test]
    fn test_validate_bucket_name_invalid_start_end() {
        assert!(validate_bucket_name("-bucket").is_err());
        assert!(validate_bucket_name("bucket-").is_err());
        assert!(validate_bucket_name(".bucket").is_err());
        assert!(validate_bucket_name("bucket.").is_err());
    }
}
