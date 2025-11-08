use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures::TryStreamExt;
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::{MetadataError, ObjectMetadata};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::io::StreamReader;
use tracing::{debug, error, info, instrument};
use uuid::Uuid;

use crate::handlers::ApiError;
use crate::metrics::{multipart_uploads_in_progress, object_size_bytes};
use crate::state::AppState;

#[derive(Debug, Default, Deserialize)]
pub struct InitiateQuery {
    pub uploads: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UploadPartQuery {
    #[serde(rename = "partNumber")]
    pub part_number: u32,
    #[serde(rename = "uploadId")]
    pub upload_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CompleteQuery {
    #[serde(rename = "uploadId")]
    pub upload_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MultipartQueryParams {
    #[serde(rename = "partNumber")]
    pub part_number: Option<u32>,
    #[serde(rename = "uploadId")]
    pub upload_id: Option<String>,
}

#[derive(Serialize)]
pub struct InitiateResponse {
    upload_id: String,
}

#[derive(Serialize)]
pub struct UploadPartResponse {
    part_number: u32,
    etag: String,
}

#[derive(Serialize)]
pub struct CompleteResponse {
    etag: String,
}

fn part_path(data_path: &str, upload_id: &str, part_number: u32) -> PathBuf {
    PathBuf::from(data_path)
        .join("temp")
        .join("parts")
        .join(upload_id)
        .join(part_number.to_string())
}

struct MultipartCleanupGuard {
    temp_file: Option<PathBuf>,
    parts_dir: PathBuf,
    armed: bool,
}

impl MultipartCleanupGuard {
    fn new(temp_file: PathBuf, parts_dir: PathBuf) -> Self {
        Self {
            temp_file: Some(temp_file),
            parts_dir,
            armed: true,
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for MultipartCleanupGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        if let Some(temp_path) = &self.temp_file {
            let _ = std::fs::remove_file(temp_path);
        }

        let _ = std::fs::remove_dir_all(&self.parts_dir);
    }
}

struct MultipartGaugeGuard {
    armed: bool,
}

impl MultipartGaugeGuard {
    fn new() -> Self {
        multipart_uploads_in_progress().inc();
        Self { armed: true }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for MultipartGaugeGuard {
    fn drop(&mut self) {
        if self.armed {
            multipart_uploads_in_progress().dec();
        }
    }
}

#[instrument(skip(state, headers), fields(bucket = %bucket, key = %key))]
pub async fn initiate_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(_query): Query<InitiateQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Initiate multipart upload started");

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

    let upload_id = Uuid::new_v4().to_string();

    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let gauge_guard = MultipartGaugeGuard::new();

    state
        .metadata
        .initiate_multipart_upload(&bucket, &key, &upload_id, content_type)
        .await
        .map_err(|e| {
            error!("Failed to initiate multipart upload: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let parts_dir = part_path(&state.config.storage.data_path, &upload_id, 0)
        .parent()
        .unwrap()
        .to_path_buf();
    fs::create_dir_all(&parts_dir).await.map_err(|e| {
        error!("Failed to create parts directory: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    gauge_guard.disarm();

    let duration = start.elapsed();
    info!(
        "Initiate multipart upload completed in {:?}, upload_id: {}",
        duration, upload_id
    );

    Ok((
        StatusCode::OK,
        Json(InitiateResponse { upload_id }),
    )
        .into_response())
}

#[instrument(skip(state, body), fields(bucket = %bucket, key = %key, part_number = query.part_number, upload_id = %query.upload_id))]
pub async fn upload_part(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<UploadPartQuery>,
    body: Body,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Upload part started");

    validate_bucket_name(&bucket)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

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

    let stream = body
        .into_data_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
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

#[instrument(skip(state), fields(bucket = %bucket, key = %key, upload_id = %query.upload_id))]
pub async fn complete_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<CompleteQuery>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Complete multipart upload started");

    validate_bucket_name(&bucket)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

    let upload = state
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

    if upload.parts.is_empty() {
        return Err(ApiError::InvalidRequest(
            "Cannot complete upload with no parts".to_string(),
        ));
    }

    let temp_final_path = PathBuf::from(&state.config.storage.data_path)
        .join("temp")
        .join(format!("complete-{}", query.upload_id));

    let parts_dir = part_path(&state.config.storage.data_path, &query.upload_id, 0)
        .parent()
        .unwrap()
        .to_path_buf();

    let mut temp_file = fs::File::create(&temp_final_path).await.map_err(|e| {
        error!("Failed to create temp final file: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    let cleanup_guard = MultipartCleanupGuard::new(temp_final_path.clone(), parts_dir);

    let mut hasher = Sha256::new();
    let mut total_size = 0u64;

    let mut buffer = vec![0u8; 64 * 1024];

    for (part_num, _part) in &upload.parts {
        let part_file_path = part_path(
            &state.config.storage.data_path,
            &query.upload_id,
            *part_num,
        );

        let mut part_file = fs::File::open(&part_file_path).await.map_err(|e| {
            error!("Failed to open part {}: {}", part_num, e);
            ApiError::Internal(format!("Missing part {}", part_num))
        })?;

        loop {
            let n = tokio::io::AsyncReadExt::read(&mut part_file, &mut buffer)
                .await
                .map_err(|e| {
                    error!("Failed to read part {}: {}", part_num, e);
                    ApiError::Internal(format!("I/O error reading part {}", part_num))
                })?;

            if n == 0 {
                break;
            }

            hasher.update(&buffer[..n]);
            total_size += n as u64;

            temp_file.write_all(&buffer[..n]).await.map_err(|e| {
                error!("Failed to write to temp file: {}", e);
                ApiError::Internal(format!("Storage error: {}", e))
            })?;
        }
    }

    temp_file.flush().await.map_err(|e| {
        error!("Failed to flush temp file: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;
    drop(temp_file);

    let final_etag = format!("{:x}", hasher.finalize());

    let full_key = crate::handlers::objects::storage_key(&bucket, &key);
    let mut file_reader = fs::File::open(&temp_final_path).await.map_err(|e| {
        error!("Failed to open temp file for storage: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    state.storage.put_object(&full_key, &mut file_reader).await.map_err(|e| {
        error!("Failed to store object: {}", e);
        ApiError::Internal(format!("Storage error: {}", e))
    })?;

    let mut metadata = ObjectMetadata::new(bucket.clone(), key.clone(), total_size, final_etag.clone());
    metadata.content_type = upload.content_type;

    state
        .metadata
        .put_object_metadata(metadata)
        .await
        .map_err(|e| {
            error!("Failed to store object metadata: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    state
        .metadata
        .abort_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| {
            error!("Failed to clean up multipart metadata: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let parts_dir_clone = cleanup_guard.parts_dir.clone();
    cleanup_guard.disarm();

    if let Err(e) = fs::remove_dir_all(&parts_dir_clone).await {
        error!("Failed to clean up part files: {}", e);
    }

    if let Err(e) = fs::remove_file(&temp_final_path).await {
        error!("Failed to clean up temp final file: {}", e);
    }

    multipart_uploads_in_progress().dec();
    object_size_bytes()
        .with_label_values(&["multipart_complete"])
        .observe(total_size as f64);

    let duration = start.elapsed();
    info!(
        "Complete multipart upload in {:?}, total size: {} bytes, etag: {}",
        duration, total_size, final_etag
    );

    Ok((StatusCode::OK, Json(CompleteResponse { etag: final_etag })).into_response())
}

#[instrument(skip(state), fields(bucket = %bucket, key = %key, upload_id = %query.upload_id))]
pub async fn abort_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<CompleteQuery>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Abort multipart upload started");

    validate_bucket_name(&bucket)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key)
        .map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

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

    state
        .metadata
        .abort_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| {
            error!("Failed to abort multipart upload: {}", e);
            ApiError::Internal(format!("Metadata error: {}", e))
        })?;

    let parts_dir = part_path(&state.config.storage.data_path, &query.upload_id, 0)
        .parent()
        .unwrap()
        .to_path_buf();
    if let Err(e) = fs::remove_dir_all(&parts_dir).await {
        error!("Failed to clean up part files: {}", e);
    }

    multipart_uploads_in_progress().dec();

    let duration = start.elapsed();
    info!("Abort multipart upload completed in {:?}", duration);

    Ok(StatusCode::NO_CONTENT.into_response())
}
