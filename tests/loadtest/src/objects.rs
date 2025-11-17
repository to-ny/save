use rand::Rng;

#[derive(Clone)]
pub struct ObjectGenerator {
    key_prefix: String,
}

impl ObjectGenerator {
    pub fn new(key_prefix: impl Into<String>) -> Self {
        Self {
            key_prefix: key_prefix.into(),
        }
    }

    pub fn random_key(&self) -> String {
        format!("{}/{}", self.key_prefix, uuid::Uuid::new_v4())
    }

    pub fn random_data(&self, size: usize) -> Vec<u8> {
        let mut rng = rand::rng();
        (0..size).map(|_| rng.random::<u8>()).collect()
    }

    pub fn content_hash(&self, data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_key() {
        let generator = ObjectGenerator::new("test");
        let key1 = generator.random_key();
        let key2 = generator.random_key();
        assert!(key1.starts_with("test/"));
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_random_data() {
        let generator = ObjectGenerator::new("test");
        let data = generator.random_data(1024);
        assert_eq!(data.len(), 1024);
    }

    #[test]
    fn test_content_hash() {
        let generator = ObjectGenerator::new("test");
        let data = b"hello world";
        let hash = generator.content_hash(data);
        assert_eq!(hash.len(), 64);
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
