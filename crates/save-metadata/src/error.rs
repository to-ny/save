use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("Database error: {0}")]
    Database(#[from] rocksdb::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Bucket not found: {0}")]
    BucketNotFound(String),

    #[error("Bucket already exists: {0}")]
    BucketAlreadyExists(String),

    #[error("Object not found: {bucket}/{key}")]
    ObjectNotFound { bucket: String, key: String },

    #[error("Multipart upload not found: {0}")]
    MultipartUploadNotFound(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Task cancelled: {0}")]
    TaskCancelled(String),
}

pub type Result<T> = std::result::Result<T, MetadataError>;
