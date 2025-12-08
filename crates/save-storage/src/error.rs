use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Object not found: {0}")]
    NotFound(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Quorum not achieved: {achieved} of {required} nodes")]
    QuorumNotAchieved { achieved: usize, required: usize },

    #[error("TLS error: {0}")]
    Tls(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;
