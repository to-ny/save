use crate::error::{Result, StorageError};
use save_common::validate_object_key;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) struct StorageLayout {
    base_path: PathBuf,
}

impl StorageLayout {
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        Self {
            base_path: base_path.as_ref().to_path_buf(),
        }
    }

    pub fn object_id(&self, key: &str) -> Result<String> {
        validate_object_key(key).map_err(|e| StorageError::InvalidKey(e.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }

    pub fn object_path(&self, key: &str) -> Result<PathBuf> {
        let id = self.object_id(key)?;
        Ok(self.object_path_from_id(&id))
    }

    pub fn object_path_from_id(&self, id: &str) -> PathBuf {
        let prefix = &id[..2];
        self.base_path.join("objects").join(prefix).join(id)
    }

    pub fn temp_path_from_id(&self, id: &str) -> PathBuf {
        self.base_path.join("temp").join(format!("{}.tmp", id))
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.base_path.join("objects")
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.base_path.join("temp")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_key_valid() {
        assert!(validate_object_key("bucket/key").is_ok());
        assert!(validate_object_key("bucket/path/to/object").is_ok());
        assert!(validate_object_key("a").is_ok());
    }

    #[test]
    fn test_validate_key_path_traversal() {
        assert!(validate_object_key("../etc/passwd").is_err());
        assert!(validate_object_key("bucket/../key").is_err());
        assert!(validate_object_key("..").is_err());
    }

    #[test]
    fn test_validate_key_empty() {
        assert!(validate_object_key("").is_err());
    }

    #[test]
    fn test_validate_key_slashes() {
        assert!(validate_object_key("/bucket/key").is_err());
        assert!(validate_object_key("bucket/key/").is_err());
        assert!(validate_object_key("/").is_err());
    }

    #[test]
    fn test_validate_key_null_byte() {
        assert!(validate_object_key("bucket/key\0").is_err());
        assert!(validate_object_key("\0").is_err());
    }

    #[test]
    fn test_object_id_deterministic() {
        let layout = StorageLayout::new("/tmp");
        let id1 = layout.object_id("bucket/key").unwrap();
        let id2 = layout.object_id("bucket/key").unwrap();
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 64); // SHA256 hex = 64 chars
    }

    #[test]
    fn test_object_id_different_keys() {
        let layout = StorageLayout::new("/tmp");
        let id1 = layout.object_id("bucket/key1").unwrap();
        let id2 = layout.object_id("bucket/key2").unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_object_path_sharding() {
        let layout = StorageLayout::new("/data");
        let path = layout.object_path("bucket/key").unwrap();
        let id = layout.object_id("bucket/key").unwrap();
        let prefix = &id[..2];

        assert!(
            path.to_str()
                .unwrap()
                .contains(&format!("objects/{}/{}", prefix, id))
        );
        assert!(path.starts_with("/data"));
    }

    #[test]
    fn test_temp_path_format() {
        let layout = StorageLayout::new("/data");
        let id = layout.object_id("bucket/key").unwrap();
        let temp_path = layout.temp_path_from_id(&id);

        assert!(temp_path.to_str().unwrap().contains("temp"));
        assert!(temp_path.to_str().unwrap().ends_with(".tmp"));
        assert!(temp_path.to_str().unwrap().contains(&id));
    }

    #[test]
    fn test_directories() {
        let layout = StorageLayout::new("/data");
        assert_eq!(
            layout.objects_dir(),
            std::path::PathBuf::from("/data/objects")
        );
        assert_eq!(layout.temp_dir(), std::path::PathBuf::from("/data/temp"));
    }
}
