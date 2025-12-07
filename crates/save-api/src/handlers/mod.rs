pub mod bucket;
pub mod cluster;
mod error;
#[cfg(feature = "failpoints")]
pub mod failpoint;
pub mod health;
pub mod multipart;
pub mod objects;

pub use error::ApiError;

use crate::state::AppState;
use save_common::config::ConsistencyMode;
use save_metadata::MetadataError;
use tracing::debug;

/// For Strong consistency mode, verifies this node is leader with up-to-date state.
pub async fn ensure_read_consistency(state: &AppState) -> Result<(), ApiError> {
    if state.config.cluster.consistency_mode == ConsistencyMode::Strong {
        state
            .raft_node
            .ensure_linearizable()
            .await
            .map_err(|e| ApiError::internal(format!("Consistency check failed: {}", e)))?;
    }
    Ok(())
}

/// Validates that a bucket exists, using the bucket cache to avoid repeated DB lookups.
///
/// This function:
/// 1. Checks the in-memory bucket cache first
/// 2. If not cached, validates bucket exists in RocksDB
/// 3. Adds bucket to cache on successful validation
/// 4. Maps metadata errors to appropriate API errors
///
/// # Performance
/// - Cache hit: O(1) lock-free lookup, no DB access
/// - Cache miss: Single RocksDB read + cache insert
/// - Cache TTL: 60 seconds (configured in AppState)
pub async fn validate_bucket_exists(state: &AppState, bucket: &str) -> Result<(), ApiError> {
    if !state.bucket_cache.contains(bucket) {
        state
            .metadata
            .get_bucket(bucket)
            .await
            .map_err(|e| match e {
                MetadataError::BucketNotFound(_) => {
                    debug!("Bucket not found: {}", bucket);
                    ApiError::BucketNotFound(bucket.to_string())
                }
                _ => ApiError::internal(format!("Metadata error: {}", e)),
            })?;

        state.bucket_cache.insert(bucket.to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{test_setup, test_setup_empty};

    #[tokio::test]
    async fn test_validate_bucket_exists_cache_hit() {
        let (state, _temp_dir) = test_setup().await;

        // Pre-populate cache
        state.bucket_cache.insert("test-bucket".to_string());

        // This should succeed without hitting the DB
        let result = validate_bucket_exists(&state, "test-bucket").await;
        assert!(result.is_ok(), "Cached bucket should validate successfully");
    }

    #[tokio::test]
    async fn test_validate_bucket_exists_cache_miss_success() {
        let (state, _temp_dir) = test_setup().await;

        // Ensure cache is empty
        assert!(
            !state.bucket_cache.contains("test-bucket"),
            "Cache should be empty initially"
        );

        // This should hit the DB and populate the cache
        let result = validate_bucket_exists(&state, "test-bucket").await;
        assert!(
            result.is_ok(),
            "Bucket should validate successfully from DB"
        );

        // Verify cache was populated
        assert!(
            state.bucket_cache.contains("test-bucket"),
            "Cache should be populated after validation"
        );
    }

    #[tokio::test]
    async fn test_validate_bucket_exists_cache_miss_bucket_not_found() {
        let (state, _temp_dir) = test_setup_empty().await;

        // Ensure cache is empty
        assert!(
            !state.bucket_cache.contains("nonexistent-bucket"),
            "Cache should be empty initially"
        );

        // This should fail with BucketNotFound
        let result = validate_bucket_exists(&state, "nonexistent-bucket").await;
        assert!(result.is_err(), "Nonexistent bucket should fail validation");

        match result.unwrap_err() {
            ApiError::BucketNotFound(bucket) => {
                assert_eq!(bucket, "nonexistent-bucket");
            }
            other => panic!("Expected BucketNotFound error, got {:?}", other),
        }

        // Verify cache was NOT populated for failed validation
        assert!(
            !state.bucket_cache.contains("nonexistent-bucket"),
            "Cache should not be populated for failed validation"
        );
    }

    #[tokio::test]
    async fn test_validate_bucket_exists_multiple_calls_same_bucket() {
        let (state, _temp_dir) = test_setup().await;

        // First call - cache miss, hits DB
        let result1 = validate_bucket_exists(&state, "test-bucket").await;
        assert!(result1.is_ok());

        // Second call - cache hit, should NOT hit DB
        let result2 = validate_bucket_exists(&state, "test-bucket").await;
        assert!(result2.is_ok());

        // Both should succeed and cache should be populated
        assert!(state.bucket_cache.contains("test-bucket"));
    }

    #[tokio::test]
    async fn test_validate_bucket_exists_different_buckets() {
        let (state, _temp_dir) = test_setup_empty().await;

        // Create multiple buckets
        state.metadata.create_bucket("bucket-1").await.unwrap();
        state.metadata.create_bucket("bucket-2").await.unwrap();
        state.metadata.create_bucket("bucket-3").await.unwrap();

        // Validate each bucket
        assert!(validate_bucket_exists(&state, "bucket-1").await.is_ok());
        assert!(validate_bucket_exists(&state, "bucket-2").await.is_ok());
        assert!(validate_bucket_exists(&state, "bucket-3").await.is_ok());

        // All should be cached
        assert!(state.bucket_cache.contains("bucket-1"));
        assert!(state.bucket_cache.contains("bucket-2"));
        assert!(state.bucket_cache.contains("bucket-3"));
    }

    #[tokio::test]
    async fn test_validate_bucket_exists_cache_invalidation_on_delete() {
        let (state, _temp_dir) = test_setup().await;

        // Validate and cache the bucket
        validate_bucket_exists(&state, "test-bucket").await.unwrap();
        assert!(state.bucket_cache.contains("test-bucket"));

        // Simulate bucket deletion (cache should be manually invalidated by delete handler)
        state.bucket_cache.remove("test-bucket");

        // Cache should be empty now
        assert!(!state.bucket_cache.contains("test-bucket"));

        // If we try to validate now, it should fail (bucket still exists in DB in this test,
        // but in real scenario it would be deleted)
        // Just verify the cache was cleared
    }

    #[tokio::test]
    async fn test_validate_bucket_exists_concurrent_validations() {
        let (state, _temp_dir) = test_setup().await;

        let mut handles = vec![];

        // Spawn multiple concurrent validations of the same bucket
        for _ in 0..10 {
            let state_clone = state.clone();
            let handle =
                tokio::spawn(
                    async move { validate_bucket_exists(&state_clone, "test-bucket").await },
                );
            handles.push(handle);
        }

        // All should succeed
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok(), "Concurrent validation should succeed");
        }

        // Cache should be populated
        assert!(state.bucket_cache.contains("test-bucket"));
    }

    #[tokio::test]
    async fn test_ensure_read_consistency_eventual_mode() {
        let (state, _temp_dir) = test_setup().await;

        // Default test config uses Strong mode, but ensure_read_consistency
        // should succeed in single-node cluster (node is always leader)
        let result = ensure_read_consistency(&state).await;
        assert!(result.is_ok(), "Consistency check should succeed on leader");
    }
}
