mod bucket;
mod error;
mod keys;
mod multipart;
mod object;
pub mod raft;

#[cfg(test)]
mod tests;

/// Column family names used by the metadata store.
/// These are shared between the store and any external tools that need to open the database.
pub mod column_families {
    /// Default column family for object/bucket metadata
    pub const DEFAULT: &str = "default";
    /// Raft log entries
    pub const RAFT_LOG: &str = "raft_log";
    /// Raft hard state (term, vote, commit index)
    pub const RAFT_STATE: &str = "raft_state";
    /// Raft snapshots
    pub const RAFT_SNAPSHOT: &str = "raft_snapshot";

    /// All column families used by the metadata store
    pub const ALL: &[&str] = &[DEFAULT, RAFT_LOG, RAFT_STATE, RAFT_SNAPSHOT];

    /// Raft-specific column families (excludes default)
    pub const RAFT_ONLY: &[&str] = &[RAFT_LOG, RAFT_STATE, RAFT_SNAPSHOT];
}

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
    /// Creates a new MetadataStore with default configuration.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::new_with_config(path, &save_common::config::MetadataConfig::default())
    }

    /// Creates a new MetadataStore with custom configuration.
    pub fn new_with_config<P: AsRef<Path>>(
        path: P,
        config: &save_common::config::MetadataConfig,
    ) -> Result<Self> {
        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        // Performance optimizations for high-throughput workloads

        // Configure write buffer size
        // Larger write buffers reduce compaction frequency but use more memory
        opts.set_write_buffer_size(config.write_buffer_size_mb * 1024 * 1024);
        opts.set_max_write_buffer_number(config.max_write_buffer_number);
        opts.set_min_write_buffer_number_to_merge(2);

        // Configure block-based table with cache and bloom filters
        let mut block_opts = rocksdb::BlockBasedOptions::default();

        // Block cache for read performance
        let cache = rocksdb::Cache::new_lru_cache(config.block_cache_size_mb * 1024 * 1024);
        block_opts.set_block_cache(&cache);

        // Enable bloom filters for faster key existence checks (10 bits = ~1% false positive rate)
        block_opts.set_bloom_filter(10.0, false);
        block_opts.set_block_size(16 * 1024); // 16KB blocks
        opts.set_block_based_table_factory(&block_opts);

        // Configure parallelism for background compactions
        opts.set_max_background_jobs(config.max_background_jobs);

        // Enable statistics for monitoring
        opts.enable_statistics();
        opts.set_stats_dump_period_sec(300); // Dump stats every 5 minutes

        // Define column families for Raft consensus
        let cf_opts = rocksdb::Options::default();
        let cfs = column_families::ALL
            .iter()
            .map(|name| rocksdb::ColumnFamilyDescriptor::new(*name, cf_opts.clone()));

        let db = rocksdb::DB::open_cf_descriptors(&opts, path, cfs)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Returns a shared reference to the underlying RocksDB database.
    /// Used by Raft consensus layer to share the same database instance.
    pub fn db(&self) -> Arc<rocksdb::DB> {
        Arc::clone(&self.db)
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

    /// Clears all Raft state from the database.
    ///
    /// # Warning
    ///
    /// This is destructive and should only be used for recovery from corrupted state.
    /// After clearing, the node will need to be re-initialized and will lose any
    /// uncommitted log entries.
    ///
    /// In a multi-node cluster, prefer recovering via snapshot transfer from healthy
    /// peers instead of clearing state.
    pub fn clear_raft_state(&self) -> Result<()> {
        for cf_name in column_families::RAFT_ONLY {
            if let Some(cf) = self.db.cf_handle(cf_name) {
                // Get all keys in the column family and delete them
                let mut iter = self.db.raw_iterator_cf(&cf);
                iter.seek_to_first();

                let mut keys_to_delete = Vec::new();
                while iter.valid() {
                    if let Some(key) = iter.key() {
                        keys_to_delete.push(key.to_vec());
                    }
                    iter.next();
                }

                for key in keys_to_delete {
                    self.db
                        .delete_cf(&cf, &key)
                        .map_err(|e| MetadataError::Storage(e.to_string()))?;
                }
            }
        }

        Ok(())
    }
}
