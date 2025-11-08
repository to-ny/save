use crate::error::{MetadataError, Result};
use chrono::{DateTime, Utc};
use save_common::validate_object_key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn validate_upload_id(upload_id: &str) -> Result<()> {
    if upload_id.is_empty() {
        return Err(MetadataError::InvalidOperation(
            "Upload ID cannot be empty".to_string(),
        ));
    }

    if upload_id.len() > 256 {
        return Err(MetadataError::InvalidOperation(format!(
            "Upload ID too long: {} bytes (max 256)",
            upload_id.len()
        )));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultipartPart {
    pub part_number: u32,
    pub etag: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultipartUpload {
    pub upload_id: String,
    pub bucket: String,
    pub key: String,
    pub parts: BTreeMap<u32, MultipartPart>,
    pub initiated_at: DateTime<Utc>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MultipartMetadata {
    upload_id: String,
    bucket: String,
    key: String,
    initiated_at: DateTime<Utc>,
    content_type: Option<String>,
}

impl MultipartUpload {
    pub fn new(
        upload_id: String,
        bucket: String,
        key: String,
        content_type: Option<String>,
    ) -> Self {
        Self {
            upload_id,
            bucket,
            key,
            parts: BTreeMap::new(),
            initiated_at: Utc::now(),
            content_type,
        }
    }

    pub(crate) fn db_key(bucket: &str, key: &str, upload_id: &str) -> String {
        format!("mpu:{}:{}:{}", bucket, key, upload_id)
    }

    fn part_key(bucket: &str, key: &str, upload_id: &str, part_number: u32) -> String {
        format!("mpu:{}:{}:{}:part:{}", bucket, key, upload_id, part_number)
    }
}

pub(crate) fn initiate_multipart_upload(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    upload_id: &str,
    content_type: Option<String>,
) -> Result<MultipartUpload> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;
    validate_upload_id(upload_id)?;

    let metadata = MultipartMetadata {
        upload_id: upload_id.to_string(),
        bucket: bucket.to_string(),
        key: key.to_string(),
        initiated_at: Utc::now(),
        content_type: content_type.clone(),
    };
    let db_key = MultipartUpload::db_key(bucket, key, upload_id);
    let value = bincode::serialize(&metadata)?;
    db.put(&db_key, value)?;

    Ok(MultipartUpload {
        upload_id: upload_id.to_string(),
        bucket: bucket.to_string(),
        key: key.to_string(),
        parts: BTreeMap::new(),
        initiated_at: metadata.initiated_at,
        content_type,
    })
}

pub(crate) fn get_multipart_upload(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    upload_id: &str,
) -> Result<MultipartUpload> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;
    validate_upload_id(upload_id)?;

    let db_key = MultipartUpload::db_key(bucket, key, upload_id);

    let metadata = match db.get(&db_key)? {
        Some(data) => {
            let metadata: MultipartMetadata = bincode::deserialize(&data)?;
            metadata
        }
        None => {
            return Err(MetadataError::MultipartUploadNotFound(
                upload_id.to_string(),
            ));
        }
    };

    let mut parts = BTreeMap::new();
    let part_prefix = format!("mpu:{}:{}:{}:part:", bucket, key, upload_id);
    let iter = db.prefix_iterator(&part_prefix);

    for item in iter {
        let (k, value) = item?;
        if !k.starts_with(part_prefix.as_bytes()) {
            break;
        }
        let part: MultipartPart = bincode::deserialize(&value)?;
        parts.insert(part.part_number, part);
    }

    Ok(MultipartUpload {
        upload_id: metadata.upload_id,
        bucket: metadata.bucket,
        key: metadata.key,
        parts,
        initiated_at: metadata.initiated_at,
        content_type: metadata.content_type,
    })
}

pub(crate) fn record_part(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    upload_id: &str,
    part_number: u32,
    etag: String,
    size: u64,
) -> Result<()> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;
    validate_upload_id(upload_id)?;

    let metadata_key = MultipartUpload::db_key(bucket, key, upload_id);
    if db.get(&metadata_key)?.is_none() {
        return Err(MetadataError::MultipartUploadNotFound(
            upload_id.to_string(),
        ));
    }

    let part = MultipartPart {
        part_number,
        etag,
        size,
    };

    let part_key = MultipartUpload::part_key(bucket, key, upload_id, part_number);
    let value = bincode::serialize(&part)?;
    db.put(&part_key, value)?;

    Ok(())
}

pub(crate) fn complete_multipart_upload(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    upload_id: &str,
) -> Result<MultipartUpload> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;
    validate_upload_id(upload_id)?;

    let upload = get_multipart_upload(db, bucket, key, upload_id)?;

    let metadata_key = MultipartUpload::db_key(bucket, key, upload_id);
    db.delete(&metadata_key)?;

    let part_prefix = format!("mpu:{}:{}:{}:part:", bucket, key, upload_id);
    let iter = db.prefix_iterator(&part_prefix);
    let mut keys_to_delete = Vec::new();

    for item in iter {
        let (k, _) = item?;
        if !k.starts_with(part_prefix.as_bytes()) {
            break;
        }
        keys_to_delete.push(k.to_vec());
    }

    for key in keys_to_delete {
        db.delete(&key)?;
    }

    Ok(upload)
}

pub(crate) fn abort_multipart_upload(
    db: &rocksdb::DB,
    bucket: &str,
    key: &str,
    upload_id: &str,
) -> Result<()> {
    validate_object_key(key).map_err(|e| MetadataError::InvalidOperation(e.to_string()))?;
    validate_upload_id(upload_id)?;

    let metadata_key = MultipartUpload::db_key(bucket, key, upload_id);
    db.delete(&metadata_key)?;

    let part_prefix = format!("mpu:{}:{}:{}:part:", bucket, key, upload_id);
    let iter = db.prefix_iterator(&part_prefix);
    let mut keys_to_delete = Vec::new();

    for item in iter {
        let (k, _) = item?;
        if !k.starts_with(part_prefix.as_bytes()) {
            break;
        }
        keys_to_delete.push(k.to_vec());
    }

    for key in keys_to_delete {
        db.delete(&key)?;
    }

    Ok(())
}

pub(crate) fn list_multipart_uploads(
    db: &rocksdb::DB,
    bucket: &str,
) -> Result<Vec<MultipartUpload>> {
    let prefix = format!("mpu:{}:", bucket);
    let mut uploads = Vec::new();

    let iter = db.prefix_iterator(&prefix);
    for item in iter {
        let (key, value) = item?;
        if !key.starts_with(prefix.as_bytes()) {
            break;
        }

        let key_str = String::from_utf8_lossy(&key);
        if key_str.contains(":part:") {
            continue;
        }

        let metadata: MultipartMetadata = bincode::deserialize(&value)?;

        let mut parts = BTreeMap::new();
        let part_prefix = format!(
            "mpu:{}:{}:{}:part:",
            metadata.bucket, metadata.key, metadata.upload_id
        );
        let part_iter = db.prefix_iterator(&part_prefix);

        for part_item in part_iter {
            let (k, v) = part_item?;
            if !k.starts_with(part_prefix.as_bytes()) {
                break;
            }
            let part: MultipartPart = bincode::deserialize(&v)?;
            parts.insert(part.part_number, part);
        }

        uploads.push(MultipartUpload {
            upload_id: metadata.upload_id,
            bucket: metadata.bucket,
            key: metadata.key,
            parts,
            initiated_at: metadata.initiated_at,
            content_type: metadata.content_type,
        });
    }

    Ok(uploads)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db() -> rocksdb::DB {
        let path = tempfile::tempdir().unwrap();
        rocksdb::DB::open_default(path.path()).unwrap()
    }

    #[test]
    fn test_initiate_multipart_upload() {
        let db = create_test_db();
        let upload = initiate_multipart_upload(&db, "bucket", "key", "upload123", None).unwrap();
        assert_eq!(upload.upload_id, "upload123");
        assert_eq!(upload.bucket, "bucket");
        assert_eq!(upload.key, "key");
        assert!(upload.parts.is_empty());
    }

    #[test]
    fn test_get_multipart_upload() {
        let db = create_test_db();
        let created = initiate_multipart_upload(&db, "bucket", "key", "upload123", None).unwrap();
        let fetched = get_multipart_upload(&db, "bucket", "key", "upload123").unwrap();
        assert_eq!(created, fetched);
    }

    #[test]
    fn test_record_part() {
        let db = create_test_db();
        initiate_multipart_upload(&db, "bucket", "key", "upload123", None).unwrap();

        record_part(
            &db,
            "bucket",
            "key",
            "upload123",
            1,
            "etag1".to_string(),
            1024,
        )
        .unwrap();
        record_part(
            &db,
            "bucket",
            "key",
            "upload123",
            2,
            "etag2".to_string(),
            2048,
        )
        .unwrap();

        let upload = get_multipart_upload(&db, "bucket", "key", "upload123").unwrap();
        assert_eq!(upload.parts.len(), 2);
        assert_eq!(upload.parts.get(&1).unwrap().etag, "etag1");
        assert_eq!(upload.parts.get(&2).unwrap().etag, "etag2");
    }

    #[test]
    fn test_complete_multipart_upload() {
        let db = create_test_db();
        initiate_multipart_upload(&db, "bucket", "key", "upload123", None).unwrap();
        record_part(
            &db,
            "bucket",
            "key",
            "upload123",
            1,
            "etag1".to_string(),
            1024,
        )
        .unwrap();

        let upload = complete_multipart_upload(&db, "bucket", "key", "upload123").unwrap();
        assert_eq!(upload.parts.len(), 1);

        let result = get_multipart_upload(&db, "bucket", "key", "upload123");
        assert!(matches!(
            result,
            Err(MetadataError::MultipartUploadNotFound(_))
        ));
    }

    #[test]
    fn test_abort_multipart_upload() {
        let db = create_test_db();
        initiate_multipart_upload(&db, "bucket", "key", "upload123", None).unwrap();

        abort_multipart_upload(&db, "bucket", "key", "upload123").unwrap();

        let result = get_multipart_upload(&db, "bucket", "key", "upload123");
        assert!(matches!(
            result,
            Err(MetadataError::MultipartUploadNotFound(_))
        ));
    }

    #[test]
    fn test_list_multipart_uploads() {
        let db = create_test_db();
        initiate_multipart_upload(&db, "bucket", "key1", "upload1", None).unwrap();
        initiate_multipart_upload(&db, "bucket", "key2", "upload2", None).unwrap();
        initiate_multipart_upload(&db, "other-bucket", "key3", "upload3", None).unwrap();

        let uploads = list_multipart_uploads(&db, "bucket").unwrap();
        assert_eq!(uploads.len(), 2);
        assert!(uploads.iter().all(|u| u.bucket == "bucket"));
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
    fn test_validate_upload_id_valid() {
        assert!(validate_upload_id("upload123").is_ok());
        assert!(validate_upload_id("a").is_ok());
        assert!(validate_upload_id(&"x".repeat(256)).is_ok());
    }

    #[test]
    fn test_validate_upload_id_empty() {
        let result = validate_upload_id("");
        assert!(matches!(result, Err(MetadataError::InvalidOperation(_))));
    }

    #[test]
    fn test_validate_upload_id_too_long() {
        let long_id = "x".repeat(257);
        let result = validate_upload_id(&long_id);
        assert!(matches!(result, Err(MetadataError::InvalidOperation(_))));
    }
}
