//! Virtual user simulation.

use rand::Rng;
use std::collections::VecDeque;

use crate::operations::{OperationType, Operations};
use crate::patterns::TrafficPattern;

#[derive(Debug, Clone, Copy)]
pub enum UserPersonality {
    ReaderHeavy, // 80% read
    WriterHeavy, // 80% write
    Balanced,    // 50/50
}

impl UserPersonality {
    pub fn random() -> Self {
        let mut rng = rand::rng();
        match rng.random_range(0..3) {
            0 => UserPersonality::ReaderHeavy,
            1 => UserPersonality::WriterHeavy,
            _ => UserPersonality::Balanced,
        }
    }

    pub fn read_ratio(&self) -> f64 {
        match self {
            UserPersonality::ReaderHeavy => 0.8,
            UserPersonality::WriterHeavy => 0.2,
            UserPersonality::Balanced => 0.5,
        }
    }
}

pub struct VirtualUser {
    id: u32,
    personality: UserPersonality,
    created_objects: VecDeque<String>,
    max_objects: usize,
}

impl VirtualUser {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            personality: UserPersonality::random(),
            created_objects: VecDeque::new(),
            max_objects: 1000,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn personality(&self) -> UserPersonality {
        self.personality
    }

    pub fn add_object(&mut self, key: String) {
        self.created_objects.push_back(key);
        while self.created_objects.len() > self.max_objects {
            self.created_objects.pop_front();
        }
    }

    pub fn remove_object(&mut self, key: &str) {
        self.created_objects.retain(|k| k != key);
    }

    pub fn random_object(&self) -> Option<&str> {
        if self.created_objects.is_empty() {
            return None;
        }
        let mut rng = rand::rng();
        let idx = rng.random_range(0..self.created_objects.len());
        self.created_objects.get(idx).map(|s| s.as_str())
    }

    pub fn has_objects(&self) -> bool {
        !self.created_objects.is_empty()
    }

    pub fn object_count(&self) -> usize {
        self.created_objects.len()
    }

    pub fn select_operation(&self, pattern: &TrafficPattern) -> OperationType {
        let mut rng = rand::rng();

        // Determine if this should be a read or write operation
        let should_read = match pattern {
            TrafficPattern::Mixed { .. } => pattern.should_read(),
            _ => rng.random::<f64>() < self.personality.read_ratio(),
        };

        if should_read {
            // Read operations
            if !self.has_objects() {
                // No objects to read, do a list instead
                if rng.random::<f64>() < 0.1 {
                    OperationType::ListBuckets
                } else {
                    OperationType::List
                }
            } else {
                // Choose between GET, HEAD, LIST
                let roll = rng.random::<f64>();
                if roll < 0.6 {
                    OperationType::Get
                } else if roll < 0.8 {
                    OperationType::Head
                } else if roll < 0.95 {
                    OperationType::List
                } else {
                    OperationType::ListBuckets
                }
            }
        } else {
            // Write operations
            let roll = rng.random::<f64>();
            if roll < 0.7 {
                OperationType::Put
            } else if roll < 0.85 {
                if self.has_objects() {
                    OperationType::Delete
                } else {
                    OperationType::Put
                }
            } else {
                // Multipart upload for larger objects
                OperationType::MultipartUpload
            }
        }
    }

    pub async fn execute_operation(
        &mut self,
        ops: &Operations,
        operation: OperationType,
    ) -> crate::operations::OperationResult {
        match operation {
            OperationType::Put => {
                let key = ops.random_key();
                let size = ops.random_size();
                let result = ops.put_object(&key, size).await;
                if result.success {
                    self.add_object(key);
                }
                result
            }
            OperationType::Get => {
                if let Some(key) = self.random_object() {
                    ops.get_object(key).await
                } else {
                    // No objects, do a list instead
                    ops.list_objects(None).await
                }
            }
            OperationType::Delete => {
                if let Some(key) = self.random_object().map(String::from) {
                    let result = ops.delete_object(&key).await;
                    if result.success {
                        self.remove_object(&key);
                    }
                    result
                } else {
                    // No objects, do a put instead
                    let key = ops.random_key();
                    let size = ops.random_size();
                    let result = ops.put_object(&key, size).await;
                    if result.success {
                        self.add_object(key);
                    }
                    result
                }
            }
            OperationType::Head => {
                if let Some(key) = self.random_object() {
                    ops.head_object(key).await
                } else {
                    // No objects, do a list instead
                    ops.list_objects(None).await
                }
            }
            OperationType::List => ops.list_objects(None).await,
            OperationType::ListBuckets => ops.list_buckets().await,
            OperationType::MultipartUpload => {
                let key = ops.random_key();
                // Use larger sizes for multipart (10MB - 50MB)
                let size = {
                    let mut rng = rand::rng();
                    rng.random_range(10 * 1024 * 1024..50 * 1024 * 1024)
                };
                let result = ops.multipart_upload(&key, size).await;
                if result.success {
                    self.add_object(key);
                }
                result
            }
        }
    }
}
