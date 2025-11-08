use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use save_common::{validate_bucket_name, validate_object_key};
use save_metadata::MetadataError;
use std::time::Instant;
use tokio::fs;
use tracing::{debug, error, info, instrument, warn};

use crate::handlers::ApiError;
use crate::metrics::{multipart_cleanup_failures_total, multipart_uploads_in_progress};
use crate::state::AppState;

use super::{CompleteQuery, part_path};

const MAX_CLEANUP_FAILURE_RATE: f64 = 0.1;

#[instrument(skip(state), fields(bucket = %bucket, key = %key, upload_id = %query.upload_id))]
pub async fn abort_multipart(
    State(state): State<AppState>,
    Path((bucket, key)): Path<(String, String)>,
    Query(query): Query<CompleteQuery>,
) -> Result<Response, ApiError> {
    let start = Instant::now();
    info!("Abort multipart upload started");

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
            _ => ApiError::internal(format!("Metadata error: {}", e)),
        })?;

    state
        .metadata
        .abort_multipart_upload(&bucket, &key, &query.upload_id)
        .await
        .map_err(|e| ApiError::internal(format!("Metadata error: {}", e)))?;

    cleanup_part_files(&state.config.storage.data_path, &query.upload_id, &upload).await;

    multipart_uploads_in_progress().dec();

    let duration = start.elapsed();
    info!("Abort multipart upload completed in {:?}", duration);

    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn cleanup_part_files(
    data_path: &str,
    upload_id: &str,
    upload: &save_metadata::MultipartUpload,
) {
    let mut deleted_count = 0;
    let mut error_count = 0;
    let total_parts = upload.parts.len();

    for part_number in upload.parts.keys() {
        let part_file_path = part_path(data_path, upload_id, *part_number);

        match fs::remove_file(&part_file_path).await {
            Ok(()) => {
                deleted_count += 1;
                debug!(
                    upload_id = %upload_id,
                    part_number = part_number,
                    path = ?part_file_path,
                    "Deleted multipart part file"
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(
                    upload_id = %upload_id,
                    part_number = part_number,
                    path = ?part_file_path,
                    "Part file not found (already deleted)"
                );
            }
            Err(e) => {
                error_count += 1;
                warn!(
                    upload_id = %upload_id,
                    part_number = part_number,
                    path = ?part_file_path,
                    error = %e,
                    "Failed to delete multipart part file"
                );
            }
        }
    }

    let part_file = part_path(data_path, upload_id, 0);
    let Some(parts_dir) = part_file.parent() else {
        warn!(
            upload_id = %upload_id,
            "Invalid part path structure, cannot determine parts directory"
        );
        return;
    };

    if let Err(e) = fs::remove_dir(parts_dir).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        warn!(
            upload_id = %upload_id,
            path = ?parts_dir,
            error = %e,
            "Failed to remove parts directory"
        );
    }

    if error_count > 0 {
        let failure_rate = error_count as f64 / total_parts as f64;
        if failure_rate > MAX_CLEANUP_FAILURE_RATE {
            multipart_cleanup_failures_total().inc();
            error!(
                upload_id = %upload_id,
                failed = error_count,
                total = total_parts,
                failure_rate = %format!("{:.1}%", failure_rate * 100.0),
                "High cleanup failure rate detected"
            );
        }
    }

    info!(
        upload_id = %upload_id,
        deleted = deleted_count,
        errors = error_count,
        "Multipart part cleanup completed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_setup;
    use save_metadata::MultipartUpload;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn test_cleanup_part_files_all_parts_exist() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_path = temp_dir.path().to_str().unwrap();

        let upload_id = "test-upload";
        let parts_dir = temp_dir.path().join("temp/parts").join(upload_id);
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let part1 = parts_dir.join("1");
        let part2 = parts_dir.join("2");
        tokio::fs::write(&part1, b"part1 data").await.unwrap();
        tokio::fs::write(&part2, b"part2 data").await.unwrap();

        let mut parts = BTreeMap::new();
        parts.insert(
            1,
            save_metadata::MultipartPart {
                part_number: 1,
                etag: "etag1".to_string(),
                size: 10,
            },
        );
        parts.insert(
            2,
            save_metadata::MultipartPart {
                part_number: 2,
                etag: "etag2".to_string(),
                size: 10,
            },
        );

        let upload = MultipartUpload {
            upload_id: upload_id.to_string(),
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            parts,
            initiated_at: chrono::Utc::now(),
            content_type: None,
        };

        cleanup_part_files(data_path, upload_id, &upload).await;

        assert!(!part1.exists(), "Part 1 should be deleted");
        assert!(!part2.exists(), "Part 2 should be deleted");
    }

    #[tokio::test]
    async fn test_cleanup_part_files_missing_parts() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_path = temp_dir.path().to_str().unwrap();

        let upload_id = "test-upload";
        let parts_dir = temp_dir.path().join("temp/parts").join(upload_id);
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let mut parts = BTreeMap::new();
        parts.insert(
            1,
            save_metadata::MultipartPart {
                part_number: 1,
                etag: "etag1".to_string(),
                size: 10,
            },
        );
        parts.insert(
            2,
            save_metadata::MultipartPart {
                part_number: 2,
                etag: "etag2".to_string(),
                size: 10,
            },
        );

        let upload = MultipartUpload {
            upload_id: upload_id.to_string(),
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            parts,
            initiated_at: chrono::Utc::now(),
            content_type: None,
        };

        cleanup_part_files(data_path, upload_id, &upload).await;
    }

    #[tokio::test]
    async fn test_cleanup_part_files_idempotent() {
        let temp_dir = tempfile::tempdir().unwrap();
        let data_path = temp_dir.path().to_str().unwrap();

        let upload_id = "test-upload";
        let parts_dir = temp_dir.path().join("temp/parts").join(upload_id);
        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let part1 = parts_dir.join("1");
        tokio::fs::write(&part1, b"part1 data").await.unwrap();

        let mut parts = BTreeMap::new();
        parts.insert(
            1,
            save_metadata::MultipartPart {
                part_number: 1,
                etag: "etag1".to_string(),
                size: 10,
            },
        );

        let upload = MultipartUpload {
            upload_id: upload_id.to_string(),
            bucket: "test-bucket".to_string(),
            key: "test-key".to_string(),
            parts,
            initiated_at: chrono::Utc::now(),
            content_type: None,
        };

        cleanup_part_files(data_path, upload_id, &upload).await;
        assert!(!part1.exists(), "Part should be deleted first time");

        cleanup_part_files(data_path, upload_id, &upload).await;
    }

    #[tokio::test]
    async fn test_abort_multipart_end_to_end() {
        let (state, _temp_dir) = test_setup().await;

        let upload = state
            .metadata
            .initiate_multipart_upload("test-bucket", "test-key", "upload123", None)
            .await
            .unwrap();

        let parts_dir = std::path::PathBuf::from(&state.config.storage.data_path)
            .join("temp/parts")
            .join(&upload.upload_id);

        tokio::fs::create_dir_all(&parts_dir).await.unwrap();

        let part1 = parts_dir.join("1");
        tokio::fs::write(&part1, b"part1 data").await.unwrap();

        state
            .metadata
            .record_part(
                "test-bucket",
                "test-key",
                &upload.upload_id,
                1,
                "etag1".to_string(),
                10,
            )
            .await
            .unwrap();

        let result = abort_multipart(
            axum::extract::State(state.clone()),
            axum::extract::Path(("test-bucket".to_string(), "test-key".to_string())),
            axum::extract::Query(CompleteQuery {
                upload_id: upload.upload_id.clone(),
            }),
        )
        .await;

        assert!(result.is_ok(), "Abort should succeed");

        let metadata_result = state
            .metadata
            .get_multipart_upload("test-bucket", "test-key", &upload.upload_id)
            .await;

        assert!(
            metadata_result.is_err(),
            "Upload should be removed from metadata"
        );
    }

    #[tokio::test]
    async fn test_abort_multipart_idempotent() {
        let (state, _temp_dir) = test_setup().await;

        let upload = state
            .metadata
            .initiate_multipart_upload("test-bucket", "test-key", "upload456", None)
            .await
            .unwrap();

        let result1 = abort_multipart(
            axum::extract::State(state.clone()),
            axum::extract::Path(("test-bucket".to_string(), "test-key".to_string())),
            axum::extract::Query(CompleteQuery {
                upload_id: upload.upload_id.clone(),
            }),
        )
        .await;

        assert!(result1.is_ok(), "First abort should succeed");

        let result2 = abort_multipart(
            axum::extract::State(state.clone()),
            axum::extract::Path(("test-bucket".to_string(), "test-key".to_string())),
            axum::extract::Query(CompleteQuery {
                upload_id: upload.upload_id.clone(),
            }),
        )
        .await;

        assert!(result2.is_err(), "Second abort should fail with not found");
    }

    #[tokio::test]
    async fn test_abort_nonexistent_upload() {
        let (state, _temp_dir) = test_setup().await;

        let result = abort_multipart(
            axum::extract::State(state.clone()),
            axum::extract::Path(("test-bucket".to_string(), "test-key".to_string())),
            axum::extract::Query(CompleteQuery {
                upload_id: "nonexistent".to_string(),
            }),
        )
        .await;

        assert!(result.is_err(), "Should fail for nonexistent upload");
    }
}
