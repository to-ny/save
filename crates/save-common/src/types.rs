use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bucket {
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Bucket {
    pub fn new(name: String) -> Self {
        Self {
            name,
            created_at: Utc::now(),
        }
    }

    pub fn with_timestamp(name: String, created_at: DateTime<Utc>) -> Self {
        Self { name, created_at }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_creation() {
        let bucket = Bucket::new("test-bucket".to_string());
        assert_eq!(bucket.name, "test-bucket");
        assert!(bucket.created_at <= Utc::now());
    }

    #[test]
    fn test_bucket_serialization() {
        let bucket = Bucket::new("test-bucket".to_string());
        let json = serde_json::to_string(&bucket).unwrap();
        let deserialized: Bucket = serde_json::from_str(&json).unwrap();
        assert_eq!(bucket, deserialized);
    }
}
