use crate::{Bucket, ObjectMetadata};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockType {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockHolder {
    pub node_id: u64,
    pub lock_id: u64,
}

/// Metadata operations replicated through Raft.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    CreateBucket {
        bucket: Bucket,
    },
    DeleteBucket {
        name: String,
    },
    PutObjectMetadata {
        metadata: ObjectMetadata,
    },
    DeleteObjectMetadata {
        bucket: String,
        key: String,
    },
    AcquireLock {
        bucket: String,
        key: String,
        lock_type: LockType,
        holder: LockHolder,
    },
    ReleaseLock {
        bucket: String,
        key: String,
        holder: LockHolder,
    },
}

impl Command {
    pub fn create_bucket(bucket: Bucket) -> Self {
        Command::CreateBucket { bucket }
    }

    pub fn delete_bucket(name: String) -> Self {
        Command::DeleteBucket { name }
    }

    pub fn put_object_metadata(metadata: ObjectMetadata) -> Self {
        Command::PutObjectMetadata { metadata }
    }

    pub fn delete_object_metadata(bucket: String, key: String) -> Self {
        Command::DeleteObjectMetadata { bucket, key }
    }
}
