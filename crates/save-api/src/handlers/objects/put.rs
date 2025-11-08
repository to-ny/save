use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures::TryStreamExt;
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::{MetadataError, ObjectMetadata};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Instant;
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

use super::storage_key;

#[derive(Serialize)]
pub struct PutObjectResponse {
    etag: String,
}

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

    validate_bucket_name(&bucket)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    state.metadata.get_bucket(&bucket).await.map_err(|e| match e {
        MetadataError::BucketNotFound(_) => {
            debug!("Bucket not found: {}", bucket);
            ApiError::BucketNotFound(bucket.clone())
        }
        _ => {
            error!("Metadata error: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        }
    })?;

    let stream = body
        .into_data_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let stream_reader = StreamReader::new(stream);
    let mut hashing_reader = HashingReader::new(stream_reader);

    let full_key = storage_key(&bucket, &key);

    debug!("Writing object to storage: {}", full_key);
    state
        .storage
        .put_object(&full_key, &mut hashing_reader)
        .await
        .map_err(|e| {
            error!("Storage error: {}", e);
            ApiError::Internal(format!("Storage error: {}", e))
        })?;

    let (etag, size) = hashing_reader.finalize();

    debug!("Object written, size: {} bytes, etag: {}", size, etag);

    let metadata = ObjectMetadata::new(bucket.clone(), key.clone(), size, etag.clone());

    state
        .metadata
        .put_object_metadata(metadata)
        .await
        .map_err(|e| {
            error!("Metadata error: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let duration = start.elapsed();
    info!(
        "PUT completed in {:?}, size: {} bytes, etag: {}",
        duration, size, etag
    );

    Ok((StatusCode::OK, Json(PutObjectResponse { etag })).into_response())
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
        assert_eq!(etag, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
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
}
