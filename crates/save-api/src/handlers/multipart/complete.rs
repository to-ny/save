use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{
    CompleteMultipartUploadResult, S3XmlResponse, validate_bucket_name, validate_object_key,
};
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

use super::{CompleteQuery, MultipartCleanupGuard, part_path};

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

    let _guard = state
        .lock_manager
        .acquire_write_lock(&bucket, &key)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to acquire object lock: {}", e)))?;

    let upload = state
        .metadata
        .get_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| match e {
            MetadataError::MultipartUploadNotFound(_) => {
                debug!("Multipart upload not found: {}", query.upload_id);
                ApiError::InvalidRequest(format!("Upload ID not found: {}", query.upload_id))
            }
            _ => ApiError::internal(format!("Metadata error: {}", e)),
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

    let mut temp_file = fs::File::create(&temp_final_path)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

    let cleanup_guard = MultipartCleanupGuard::new(temp_final_path.clone(), parts_dir);

    let mut hasher = Sha256::new();
    let mut total_size = 0u64;

    let mut buffer = vec![0u8; 64 * 1024];

    for part_num in upload.parts.keys() {
        let part_file_path =
            part_path(&state.config.storage.data_path, &query.upload_id, *part_num);

        let mut part_file = fs::File::open(&part_file_path)
            .await
            .map_err(|_e| ApiError::internal(format!("Missing part {}", part_num)))?;

        loop {
            let n = tokio::io::AsyncReadExt::read(&mut part_file, &mut buffer)
                .await
                .map_err(|_e| ApiError::internal(format!("I/O error reading part {}", part_num)))?;

            if n == 0 {
                break;
            }

            hasher.update(&buffer[..n]);
            total_size += n as u64;

            temp_file
                .write_all(&buffer[..n])
                .await
                .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;
        }
    }

    temp_file
        .flush()
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;
    drop(temp_file);

    let final_etag = format!("{:x}", hasher.finalize());

    let full_key = crate::handlers::objects::storage_key(&bucket, &key);
    let mut file_reader = fs::File::open(&temp_final_path)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

    let temp_object = state
        .storage
        .write_temp_object(&full_key, &mut file_reader)
        .await
        .map_err(|e| ApiError::internal(format!("Storage error: {}", e)))?;

    // Commit storage first, then metadata (same ordering as PUT)
    state
        .storage
        .commit_object(temp_object)
        .await
        .map_err(|e| ApiError::internal(format!("Storage commit failed: {}", e)))?;

    let mut metadata =
        ObjectMetadata::new(bucket.clone(), key.clone(), total_size, final_etag.clone());
    metadata.content_type = upload.content_type;

    state
        .metadata
        .commit_object_metadata(metadata)
        .await
        .map_err(|e| ApiError::internal(format!("Metadata commit failed: {}", e)))?;

    state
        .metadata
        .abort_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| ApiError::internal(format!("Metadata error: {}", e)))?;

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

    // TODO Hardcoded protocol
    let endpoint = format!("http://{}", state.config.server.bind_address);
    let result = CompleteMultipartUploadResult::new(bucket, key, final_etag, endpoint);
    let xml = result
        .to_xml()
        .map_err(|e| ApiError::internal(format!("Failed to serialize response: {}", e)))?;

    crate::metrics::response_size_bytes()
        .with_label_values(&["complete_multipart"])
        .observe(xml.len() as f64);

    Ok((StatusCode::OK, [("content-type", "application/xml")], xml).into_response())
}
