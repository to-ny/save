use crate::{Bucket, ObjectMetadata};
use serde::{Deserialize, Serialize};

/// Metadata operations replicated through Raft.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    CreateBucket { bucket: Bucket },
    DeleteBucket { name: String },
    PutObjectMetadata { metadata: ObjectMetadata },
    DeleteObjectMetadata { bucket: String, key: String },
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
