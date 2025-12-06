use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("Database error: {0}")]
    Database(#[from] rocksdb::Error),

    #[error("Serialization encode error: {0}")]
    Encode(#[from] bincode::error::EncodeError),

    #[error("Serialization decode error: {0}")]
    Decode(#[from] bincode::error::DecodeError),

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

    #[error("Raft error: {0}")]
    Raft(String),

    #[error("Storage error: {0}")]
    Storage(String),
}

impl MetadataError {
    /// Returns true if this error indicates corrupted Raft state that may be recoverable
    /// by clearing Raft data and re-initializing.
    pub fn is_recoverable_raft_corruption(&self) -> bool {
        match self {
            // JSON deserialization errors in Raft state are recoverable
            MetadataError::Raft(msg) => {
                msg.contains("missing field")
                    || msg.contains("invalid type")
                    || msg.contains("expected")
                    || msg.contains("JSON")
                    || msg.contains("deserialize")
            }
            _ => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, MetadataError>;
