mod abort;
mod complete;
mod initiate;
mod list;
mod upload_part;

pub use abort::abort_multipart;
pub use complete::complete_multipart;
pub use initiate::initiate_multipart;
pub use list::{ListUploadsResponse, UploadInfo, list_multipart_uploads};
pub use upload_part::upload_part;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::metrics::multipart_uploads_in_progress;

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
    pub upload_id: String,
}

#[derive(Serialize)]
pub struct UploadPartResponse {
    pub part_number: u32,
    pub etag: String,
}

#[derive(Serialize)]
pub struct CompleteResponse {
    pub etag: String,
}

pub(crate) fn part_path(data_path: &str, upload_id: &str, part_number: u32) -> PathBuf {
    PathBuf::from(data_path)
        .join("temp")
        .join("parts")
        .join(upload_id)
        .join(part_number.to_string())
}

pub(crate) struct MultipartCleanupGuard {
    temp_file: Option<PathBuf>,
    parts_dir: PathBuf,
    armed: bool,
}

impl MultipartCleanupGuard {
    pub fn new(temp_file: PathBuf, parts_dir: PathBuf) -> Self {
        Self {
            temp_file: Some(temp_file),
            parts_dir,
            armed: true,
        }
    }

    pub fn disarm(mut self) {
        self.armed = false;
    }

    pub fn parts_dir(&self) -> &PathBuf {
        &self.parts_dir
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

pub(crate) struct MultipartGaugeGuard {
    armed: bool,
}

impl MultipartGaugeGuard {
    pub fn new() -> Self {
        multipart_uploads_in_progress().inc();
        Self { armed: true }
    }

    pub fn disarm(mut self) {
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
