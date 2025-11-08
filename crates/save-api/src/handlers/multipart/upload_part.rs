use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use sha2::{Digest, Sha256};
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::state::AppState;

use super::{UploadPartQuery, UploadPartResponse, part_path};

#[instrument(skip(state, body), fields(bucket = %bucket, key = %key, part_number = query.part_number, upload_id = %query.upload_id))]
pub async fn upload_part(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<UploadPartQuery>,
    body: Body,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Upload part started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    if query.part_number == 0 || query.part_number > 10000 {
        return Err(ApiError::InvalidRequest(
            "Part number must be between 1 and 10000".to_string(),
        ));
    }

    state
        .metadata
        .get_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| match e {
            MetadataError::MultipartUploadNotFound(_) => {
                debug!("Multipart upload not found: {}", query.upload_id);
                ApiError::InvalidRequest(format!("Upload ID not found: {}", query.upload_id))
            }
            _ => {
                error!("Metadata error: {}", e);
                ApiError::Internal(format!("Metadata error: {}", e))
            }
        })?;

    let part_file_path = part_path(
        &state.config.storage.data_path,
        &query.upload_id,
        query.part_number,
    );

    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let stream_reader = StreamReader::new(stream);

    let mut file = fs::File::create(&part_file_path).await.map_err(|e| {
        error!("Failed to create part file: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut reader = stream_reader;
    const MAX_PART_SIZE: u64 = 5 * 1024 * 1024 * 1024;

    let mut buffer = vec![0u8; 8192];
    loop {
        let n = tokio::io::AsyncReadExt::read(&mut reader, &mut buffer)
            .await
            .map_err(|e| {
                error!("Failed to read part data: {}", e);
                ApiError::Internal(format!("I/O error: {}", e))
            })?;

        if n == 0 {
            break;
        }

        hasher.update(&buffer[..n]);
        size += n as u64;

        if size > MAX_PART_SIZE {
            return Err(ApiError::InvalidRequest(format!(
                "Part size exceeds maximum of {} bytes (5GB)",
                MAX_PART_SIZE
            )));
        }

        file.write_all(&buffer[..n]).await.map_err(|e| {
            error!("Failed to write part data: {}", e);
            ApiError::Internal(format!("Storage error: {}", e))
        })?;
    }

    file.flush().await.map_err(|e| {
        error!("Failed to flush part file: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    let etag = format!("{:x}", hasher.finalize());

    state
        .metadata
        .record_part(
            &bucket,
            &key,
            &query.upload_id,
            query.part_number,
            etag.clone(),
            size,
        )
        .await
        .map_err(|e| {
            error!("Failed to record part: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let duration = start.elapsed();
    info!(
        "Upload part completed in {:?}, part: {}, size: {} bytes, etag: {}",
        duration, query.part_number, size, etag
    );

    Ok((
        StatusCode::OK,
        Json(UploadPartResponse {
            part_number: query.part_number,
            etag,
        }),
    )
        .into_response())
}
