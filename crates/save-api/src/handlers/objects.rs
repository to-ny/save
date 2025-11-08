use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures::TryStreamExt;
use save_metadata::ObjectMetadata;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::Instant;
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, instrument};

use crate::state::AppState;

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
) -> Result<Response, AppError> {
    let start = Instant::now();
    info!("PUT request started");

    if state.metadata.get_bucket(&bucket).await.is_err() {
        debug!("Bucket not found: {}", bucket);
        return Err(AppError::BucketNotFound(bucket));
    }

    let stream = body
        .into_data_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let stream_reader = StreamReader::new(stream);
    let mut hashing_reader = HashingReader::new(stream_reader);

    let full_key = format!("{}/{}", bucket, key);

    debug!("Writing object to storage: {}", full_key);
    state
        .storage
        .put_object(&full_key, &mut hashing_reader)
        .await
        .map_err(|e| {
            error!("Storage error: {}", e);
            AppError::Internal(format!("Storage error: {}", e))
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
            AppError::Internal(format!("Metadata error: {}", e))
        })?;

    let duration = start.elapsed();
    info!(
        "PUT completed in {:?}, size: {} bytes, etag: {}",
        duration, size, etag
    );

    Ok((StatusCode::OK, Json(PutObjectResponse { etag })).into_response())
}

pub enum AppError {
    BucketNotFound(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BucketNotFound(bucket) => {
                (StatusCode::NOT_FOUND, format!("Bucket not found: {}", bucket))
            }
            AppError::Internal(msg) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("Internal error: {}", msg))
            }
        };

        (status, message).into_response()
    }
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

    #[test]
    fn test_app_error_bucket_not_found() {
        let error = AppError::BucketNotFound("test-bucket".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_app_error_internal() {
        let error = AppError::Internal("something went wrong".to_string());
        let response = error.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
