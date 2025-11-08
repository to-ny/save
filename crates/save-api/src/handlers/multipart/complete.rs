use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::{MetadataError, ObjectMetadata};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Instant;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, instrument};

use crate::handlers::ApiError;
use crate::metrics::{multipart_uploads_in_progress, object_size_bytes};
use crate::state::AppState;

use super::{CompleteQuery, CompleteResponse, MultipartCleanupGuard, part_path};

#[instrument(skip(state), fields(bucket = %bucket, key = %key, upload_id = %query.upload_id))]
pub async fn complete_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<CompleteQuery>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Complete multipart upload started");

    validate_bucket_name(&bucket).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;
    validate_object_key(&key).map_err(|e| ApiError::InvalidRequest(e.to_string()))?;

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

    for part_num in upload.parts.keys() {
        let part_file_path =
            part_path(&state.config.storage.data_path, &query.upload_id, *part_num);

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

    state
        .storage
        .put_object(&full_key, &mut file_reader)
        .await
        .map_err(|e| {
            error!("Failed to store object: {}", e);
            ApiError::Internal(format!("Storage error: {}", e))
        })?;

    let mut metadata =
        ObjectMetadata::new(bucket.clone(), key.clone(), total_size, final_etag.clone());
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

    let parts_dir_clone = cleanup_guard.parts_dir().clone();
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
