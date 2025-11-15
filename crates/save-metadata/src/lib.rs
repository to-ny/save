mod bucket;
mod error;
mod multipart;
mod object;

#[cfg(test)]
mod tests;

pub use error::{MetadataError, Result};
pub use multipart::{MultipartPart, MultipartUpload};
pub use object::ObjectMetadata;
pub use save_common::Bucket;

use std::path::Path;
use std::sync::Arc;

/// Database statistics for monitoring.
#[derive(Debug, Clone, Default)]
pub struct DatabaseStats {
    pub block_cache_hits: Option<u64>,
    pub block_cache_misses: Option<u64>,
    pub memtable_size_bytes: Option<u64>,
    pub table_readers_mem_bytes: Option<u64>,
    pub estimate_num_keys: Option<u64>,
}

pub struct MetadataStore {
    db: Arc<rocksdb::DB>,
}

impl MetadataStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = rocksdb::DB::open_default(path)?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn get_stats(&self) -> DatabaseStats {
        DatabaseStats {
            block_cache_hits: self
                .db
                .property_int_value("rocksdb.block-cache-hit")
                .ok()
                .flatten(),
            block_cache_misses: self
                .db
                .property_int_value("rocksdb.block-cache-miss")
                .ok()
                .flatten(),
            memtable_size_bytes: self
                .db
                .property_int_value("rocksdb.cur-size-all-mem-tables")
                .ok()
                .flatten(),
            table_readers_mem_bytes: self
                .db
                .property_int_value("rocksdb.estimate-table-readers-mem")
                .ok()
                .flatten(),
            estimate_num_keys: self
                .db
                .property_int_value("rocksdb.estimate-num-keys")
                .ok()
                .flatten(),
        }
    }

    pub async fn create_bucket(&self, name: &str) -> Result<Bucket> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || bucket::create_bucket(&db, &name))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn get_bucket(&self, name: &str) -> Result<Bucket> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || bucket::get_bucket(&db, &name))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn delete_bucket(&self, name: &str) -> Result<()> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || bucket::delete_bucket(&db, &name))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn list_buckets(&self) -> Result<Vec<Bucket>> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || bucket::list_buckets(&db))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn put_object_metadata(&self, metadata: ObjectMetadata) -> Result<()> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || object::put_object_metadata(&db, &metadata))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    /// Atomically commits object metadata with WriteBatch sync.
    pub async fn commit_object_metadata(&self, metadata: ObjectMetadata) -> Result<()> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || object::commit_object_metadata(&db, &metadata))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn get_object_metadata(&self, bucket: &str, key: &str) -> Result<ObjectMetadata> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || object::get_object_metadata(&db, &bucket, &key))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn delete_object_metadata(&self, bucket: &str, key: &str) -> Result<()> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || object::delete_object_metadata(&db, &bucket, &key))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<ObjectMetadata>> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let prefix = prefix.map(|s| s.to_string());
        tokio::task::spawn_blocking(move || object::list_objects(&db, &bucket, prefix.as_deref()))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn initiate_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        content_type: Option<String>,
    ) -> Result<MultipartUpload> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        let upload_id = upload_id.to_string();
        tokio::task::spawn_blocking(move || {
            multipart::initiate_multipart_upload(&db, &bucket, &key, &upload_id, content_type)
        })
        .await
        .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn get_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<MultipartUpload> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        let upload_id = upload_id.to_string();
        tokio::task::spawn_blocking(move || {
            multipart::get_multipart_upload(&db, &bucket, &key, &upload_id)
        })
        .await
        .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn record_part(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
        part_number: u32,
        etag: String,
        size: u64,
    ) -> Result<()> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        let upload_id = upload_id.to_string();
        tokio::task::spawn_blocking(move || {
            multipart::record_part(&db, &bucket, &key, &upload_id, part_number, etag, size)
        })
        .await
        .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn complete_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<MultipartUpload> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        let upload_id = upload_id.to_string();
        tokio::task::spawn_blocking(move || {
            multipart::complete_multipart_upload(&db, &bucket, &key, &upload_id)
        })
        .await
        .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn abort_multipart_upload(
        &self,
        bucket: &str,
        key: &str,
        upload_id: &str,
    ) -> Result<()> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        let key = key.to_string();
        let upload_id = upload_id.to_string();
        tokio::task::spawn_blocking(move || {
            multipart::abort_multipart_upload(&db, &bucket, &key, &upload_id)
        })
        .await
        .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn list_multipart_uploads(&self, bucket: &str) -> Result<Vec<MultipartUpload>> {
        let db = Arc::clone(&self.db);
        let bucket = bucket.to_string();
        tokio::task::spawn_blocking(move || multipart::list_multipart_uploads(&db, &bucket))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }

    pub async fn list_all_multipart_uploads(&self) -> Result<Vec<MultipartUpload>> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || multipart::list_all_multipart_uploads(&db))
            .await
            .map_err(|e| MetadataError::TaskCancelled(e.to_string()))?
    }
}
