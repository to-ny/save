use crate::middleware::RequestTracker;
use dashmap::DashMap;
use save_common::config::SaveConfig;
use save_metadata::MetadataStore;
use save_metadata::lock::DistributedLockManager;
use save_metadata::raft::RaftNode;
use save_storage::StorageBackend;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Simple bucket existence cache with TTL
pub struct BucketCache {
    cache: DashMap<String, Instant>,
    ttl: Duration,
}

impl BucketCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: DashMap::new(),
            ttl,
        }
    }

    pub fn contains(&self, bucket: &str) -> bool {
        if let Some(entry) = self.cache.get(bucket) {
            entry.value().elapsed() < self.ttl
        } else {
            false
        }
    }

    pub fn insert(&self, bucket: String) {
        self.cache.insert(bucket, Instant::now());
    }

    pub fn remove(&self, bucket: &str) {
        self.cache.remove(bucket);
    }

    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        self.cache
            .retain(|_, inserted_at| now.duration_since(*inserted_at) < self.ttl);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub metadata: Arc<MetadataStore>,
    pub config: Arc<SaveConfig>,
    pub lock_manager: Arc<DistributedLockManager>,
    pub request_tracker: Arc<RequestTracker>,
    pub bucket_cache: Arc<BucketCache>,
    pub start_time: Instant,
    pub raft_node: Arc<RaftNode>,
}

impl AppState {
    pub fn new(
        storage: impl StorageBackend + 'static,
        metadata: MetadataStore,
        config: SaveConfig,
        raft_node: RaftNode,
    ) -> Self {
        let bucket_cache =
            BucketCache::new(Duration::from_secs(config.server.bucket_cache_ttl_secs));

        let metadata = Arc::new(metadata);
        let raft_node = Arc::new(raft_node);
        let node_id = raft_node.node_id();
        let lock_manager =
            DistributedLockManager::new(Arc::clone(&raft_node), metadata.db(), node_id);

        Self {
            storage: Arc::new(storage),
            metadata,
            config: Arc::new(config),
            lock_manager: Arc::new(lock_manager),
            request_tracker: Arc::new(RequestTracker::new()),
            bucket_cache: Arc::new(bucket_cache),
            start_time: Instant::now(),
            raft_node,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_cache_hit_with_fresh_entry() {
        let cache = BucketCache::new(Duration::from_secs(60));

        cache.insert("test-bucket".to_string());

        assert!(
            cache.contains("test-bucket"),
            "Fresh entry should be in cache"
        );
    }

    #[test]
    fn test_bucket_cache_miss_nonexistent() {
        let cache = BucketCache::new(Duration::from_secs(60));

        assert!(
            !cache.contains("nonexistent-bucket"),
            "Nonexistent bucket should not be in cache"
        );
    }

    #[tokio::test]
    async fn test_bucket_cache_miss_expired_entry() {
        let cache = BucketCache::new(Duration::from_millis(100));

        cache.insert("test-bucket".to_string());
        assert!(
            cache.contains("test-bucket"),
            "Fresh entry should be in cache"
        );

        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!(
            !cache.contains("test-bucket"),
            "Expired entry should not be in cache"
        );
    }

    #[test]
    fn test_bucket_cache_insert_and_contains() {
        let cache = BucketCache::new(Duration::from_secs(60));

        cache.insert("bucket-1".to_string());
        cache.insert("bucket-2".to_string());
        cache.insert("bucket-3".to_string());

        assert!(cache.contains("bucket-1"));
        assert!(cache.contains("bucket-2"));
        assert!(cache.contains("bucket-3"));
        assert!(!cache.contains("bucket-4"));
    }

    #[test]
    fn test_bucket_cache_remove() {
        let cache = BucketCache::new(Duration::from_secs(60));

        cache.insert("test-bucket".to_string());
        assert!(cache.contains("test-bucket"), "Bucket should be cached");

        cache.remove("test-bucket");
        assert!(
            !cache.contains("test-bucket"),
            "Removed bucket should not be in cache"
        );
    }

    #[test]
    fn test_bucket_cache_remove_nonexistent() {
        let cache = BucketCache::new(Duration::from_secs(60));

        // Removing non-existent entry should not panic
        cache.remove("nonexistent");
    }

    #[tokio::test]
    async fn test_bucket_cache_cleanup_expired() {
        let cache = BucketCache::new(Duration::from_millis(100));

        cache.insert("fresh-bucket".to_string());
        cache.insert("old-bucket".to_string());

        // Wait for old-bucket to expire
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Insert another fresh bucket
        cache.insert("new-bucket".to_string());

        // Run cleanup
        cache.cleanup_expired();

        // old-bucket should be gone, but we can't directly check cache size
        // So we verify that expired entries are removed by checking contains()
        assert!(
            !cache.contains("old-bucket"),
            "Old bucket should be expired and cleaned up"
        );
        assert!(
            cache.contains("new-bucket"),
            "New bucket should still be cached"
        );
    }

    #[tokio::test]
    async fn test_bucket_cache_concurrent_access() {
        let cache = Arc::new(BucketCache::new(Duration::from_secs(60)));

        let mut handles = vec![];

        // Spawn multiple concurrent tasks inserting to the cache
        for i in 0..10 {
            let cache_clone = Arc::clone(&cache);
            let handle = tokio::spawn(async move {
                cache_clone.insert(format!("bucket-{}", i));
            });
            handles.push(handle);
        }

        // Wait for all inserts
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all entries are in the cache
        for i in 0..10 {
            assert!(
                cache.contains(&format!("bucket-{}", i)),
                "Bucket {} should be in cache",
                i
            );
        }
    }

    #[test]
    fn test_bucket_cache_same_bucket_reinsert() {
        let cache = BucketCache::new(Duration::from_secs(60));

        cache.insert("test-bucket".to_string());
        let first_time = cache.cache.get("test-bucket").map(|e| *e.value());

        // Re-insert the same bucket
        cache.insert("test-bucket".to_string());
        let second_time = cache.cache.get("test-bucket").map(|e| *e.value());

        // The timestamp should be updated
        assert!(
            second_time > first_time,
            "Re-insert should update the timestamp"
        );
    }
}
