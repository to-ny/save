use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::ObjectMetadata;
use sha2::{Digest, Sha256};
use std::time::Instant;
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;
use tracing::{debug, info, instrument};

use crate::handlers::{ApiError, validate_bucket_exists};
use crate::metrics::{atomic_put_operations_total, object_size_bytes};
use crate::state::AppState;

use super::storage_key;

pub struct HashingReader<R> {
    inner: R,
    hasher: Sha256,
    size: u64,
}

impl<R: AsyncRead + Unpin> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            size: 0,
        }
    }

    fn finalize(self) -> (String, u64) {
        let hash = self.hasher.finalize();
        let etag = format!("{:x}", hash);
        (etag, self.size)
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for HashingReader<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before_len = buf.filled().len();
        let result = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);

        if let std::task::Poll::Ready(Ok(())) = &result {
            let after_len = buf.filled().len();
            let new_data = &buf.filled()[before_len..after_len];
            self.hasher.update(new_data);
            self.size += (after_len - before_len) as u64;
        }

        result
    }
}

#[instrument(skip(state, body), fields(bucket = %bucket, key = %key))]
pub async fn put_object(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    body: Body,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("PUT request started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    let _guard = state
        .lock_manager
        .acquire_write_lock(&bucket, &key)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to acquire object lock: {}", e)))?;

    validate_bucket_exists(&state, &bucket).await?;

    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let stream_reader = StreamReader::new(stream);
    let mut hashing_reader = HashingReader::new(stream_reader);

    let full_key = storage_key(&bucket, &key);

    let temp_object = state
        .storage
        .write_temp_object(&full_key, &mut hashing_reader)
        .await
        .map_err(|e| {
            atomic_put_operations_total()
                .with_label_values(&["temp_write", "error"])
                .inc();
            ApiError::internal(format!("Storage error: {}", e))
        })?;

    atomic_put_operations_total()
        .with_label_values(&["temp_write", "success"])
        .inc();

    let (etag, size) = hashing_reader.finalize();

    debug!(
        size = size,
        etag = %etag,
        "Object written to temp and synced"
    );

    let metadata = ObjectMetadata::new(bucket.clone(), key.clone(), size, etag.clone());

    // Commit storage first, then metadata. This ensures metadata never points to
    // non-existent storage. Orphaned files (if metadata commit fails) are GC'd.
    if let Err(e) = state.storage.commit_object(temp_object).await {
        atomic_put_operations_total()
            .with_label_values(&["storage_commit", "error"])
            .inc();
        return Err(ApiError::internal(format!("Storage commit failed: {}", e)));
    }

    atomic_put_operations_total()
        .with_label_values(&["storage_commit", "success"])
        .inc();

    if let Err(e) = state.metadata.commit_object_metadata(metadata).await {
        atomic_put_operations_total()
            .with_label_values(&["metadata_commit", "error"])
            .inc();
        return Err(ApiError::internal(format!(
            "Metadata commit failed after storage commit - orphaned object may require GC: {}",
            e
        )));
    }

    atomic_put_operations_total()
        .with_label_values(&["metadata_commit", "success"])
        .inc();

    object_size_bytes()
        .with_label_values(&["put"])
        .observe(size as f64);

    let duration = start.elapsed();
    info!(
        "PUT completed in {:?}, size: {} bytes, etag: {}",
        duration, size, etag
    );

    Ok((StatusCode::OK, [("etag", format!("\"{}\"", etag).as_str())]).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_hashing_reader_empty() {
        let data: &[u8] = b"";
        let mut reader = HashingReader::new(data);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();

        let (etag, size) = reader.finalize();

        assert_eq!(size, 0);
        assert_eq!(
            etag,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn test_hashing_reader_with_data() {
        let data = b"Hello, World!";
        let mut reader = HashingReader::new(&data[..]);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();

        let (etag, size) = reader.finalize();

        assert_eq!(size, 13);
        assert_eq!(buf, b"Hello, World!");

        let expected_hash = format!("{:x}", Sha256::digest(b"Hello, World!"));
        assert_eq!(etag, expected_hash);
    }

    #[tokio::test]
    async fn test_hashing_reader_large_data() {
        let data = vec![0u8; 10_000];
        let mut reader = HashingReader::new(&data[..]);
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();

        let (etag, size) = reader.finalize();

        assert_eq!(size, 10_000);
        assert_eq!(buf.len(), 10_000);

        let expected_hash = format!("{:x}", Sha256::digest(&data));
        assert_eq!(etag, expected_hash);
    }

    #[tokio::test]
    async fn test_atomic_put_metadata_storage_consistency() {
        use crate::test_helpers::test_setup;

        let (state, _temp_dir) = test_setup().await;

        let data = b"atomic test data";
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/test-bucket/test-object.txt")
            .body(axum::body::Body::from(&data[..]))
            .unwrap();

        let (state_extract, path_extract, body) = (
            axum::extract::State(state.clone()),
            axum::extract::Path(("test-bucket".to_string(), "test-object.txt".to_string())),
            request.into_body(),
        );

        let result = put_object(state_extract, path_extract, body).await;
        assert!(result.is_ok(), "PUT should succeed");

        let metadata = state
            .metadata
            .get_object_metadata("test-bucket", "test-object.txt")
            .await
            .unwrap();
        assert_eq!(metadata.size, data.len() as u64);

        let mut file = state
            .storage
            .get_object(&storage_key("test-bucket", "test-object.txt"))
            .await
            .unwrap();

        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, data);
    }

    #[tokio::test]
    async fn test_atomic_put_raii_cleanup() {
        use crate::test_helpers::test_setup;

        let (state, _temp_dir) = test_setup().await;

        let data = b"test data for RAII cleanup";
        let storage_key_val = storage_key("test-bucket", "test-object.txt");

        let temp_path = {
            let temp_object = state
                .storage
                .write_temp_object(&storage_key_val, &data[..])
                .await
                .unwrap();

            let path = temp_object.temp_path().to_path_buf();
            assert!(path.exists(), "Temp file should exist");

            path
        };

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        assert!(
            !temp_path.exists(),
            "Temp file should be auto-cleaned up via RAII Drop"
        );
    }

    #[tokio::test]
    async fn test_atomic_put_concurrent_same_key() {
        use crate::test_helpers::test_setup;

        let (state, _temp_dir) = test_setup().await;

        let data1 = b"first write";
        let data2 = b"second write wins";

        let state1 = state.clone();
        let state2 = state.clone();

        let handle1 = tokio::spawn(async move {
            let request = axum::http::Request::builder()
                .method("PUT")
                .uri("/test-bucket/concurrent.txt")
                .body(axum::body::Body::from(&data1[..]))
                .unwrap();

            let (state_extract, path_extract, body) = (
                axum::extract::State(state1),
                axum::extract::Path(("test-bucket".to_string(), "concurrent.txt".to_string())),
                request.into_body(),
            );

            put_object(state_extract, path_extract, body).await
        });

        let handle2 = tokio::spawn(async move {
            let request = axum::http::Request::builder()
                .method("PUT")
                .uri("/test-bucket/concurrent.txt")
                .body(axum::body::Body::from(&data2[..]))
                .unwrap();

            let (state_extract, path_extract, body) = (
                axum::extract::State(state2),
                axum::extract::Path(("test-bucket".to_string(), "concurrent.txt".to_string())),
                request.into_body(),
            );

            put_object(state_extract, path_extract, body).await
        });

        let result1 = handle1.await.unwrap();
        let result2 = handle2.await.unwrap();

        // With locking, exactly one should succeed (they're serialized)
        // Both succeed is also acceptable if one overwrites the other
        assert!(
            result1.is_ok() || result2.is_ok(),
            "At least one PUT should succeed"
        );

        let metadata = state
            .metadata
            .get_object_metadata("test-bucket", "concurrent.txt")
            .await
            .unwrap();

        assert!(
            metadata.size == data1.len() as u64 || metadata.size == data2.len() as u64,
            "Object should have one of the written sizes"
        );

        // Verify content integrity
        let mut file = state
            .storage
            .get_object(&storage_key("test-bucket", "concurrent.txt"))
            .await
            .unwrap();

        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await.unwrap();

        assert!(
            contents == data1 || contents == data2,
            "Content must match one of the written payloads"
        );

        let actual_hash = format!("{:x}", Sha256::digest(&contents));
        assert_eq!(metadata.etag, actual_hash, "ETag must match content hash");
        assert_eq!(metadata.size, contents.len() as u64, "Size must match");
    }

    #[tokio::test]
    async fn test_atomic_put_invalid_bucket() {
        use crate::test_helpers::test_setup_empty;

        let (state, _temp_dir) = test_setup_empty().await;

        let data = b"test data";
        let request = axum::http::Request::builder()
            .method("PUT")
            .uri("/nonexistent-bucket/test-object.txt")
            .body(axum::body::Body::from(&data[..]))
            .unwrap();

        let (state_extract, path_extract, body) = (
            axum::extract::State(state.clone()),
            axum::extract::Path((
                "nonexistent-bucket".to_string(),
                "test-object.txt".to_string(),
            )),
            request.into_body(),
        );

        let result = put_object(state_extract, path_extract, body).await;

        assert!(result.is_err(), "PUT to nonexistent bucket should fail");
        assert!(
            matches!(result.unwrap_err(), ApiError::BucketNotFound(_)),
            "Should return BucketNotFound error"
        );
    }
}
