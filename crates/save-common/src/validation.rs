use crate::error::{Error, Result};

pub const MAX_OBJECT_KEY_LENGTH: usize = 1024;
pub const MIN_BUCKET_NAME_LENGTH: usize = 3;
pub const MAX_BUCKET_NAME_LENGTH: usize = 63;

pub fn validate_object_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(Error::validation("Object key cannot be empty"));
    }

    if key.len() > MAX_OBJECT_KEY_LENGTH {
        return Err(Error::validation(format!(
            "Object key too long: {} bytes (max {})",
            key.len(),
            MAX_OBJECT_KEY_LENGTH
        )));
    }

    if key.contains('\0') {
        return Err(Error::validation("Object key cannot contain null bytes"));
    }

    if key.contains("..") {
        return Err(Error::validation("Object key cannot contain '..'"));
    }

    if key.starts_with('/') || key.ends_with('/') {
        return Err(Error::validation("Object key cannot start or end with '/'"));
    }

    Ok(())
}

pub fn validate_bucket_name(name: &str) -> Result<()> {
    if name.len() < MIN_BUCKET_NAME_LENGTH || name.len() > MAX_BUCKET_NAME_LENGTH {
        return Err(Error::validation(format!(
            "Bucket name must be {}-{} characters, got {}",
            MIN_BUCKET_NAME_LENGTH,
            MAX_BUCKET_NAME_LENGTH,
            name.len()
        )));
    }

    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err(Error::validation(
            "Bucket name must contain only lowercase letters, numbers, hyphens, and dots",
        ));
    }

    if name.starts_with('-') || name.starts_with('.') || name.ends_with('-') || name.ends_with('.')
    {
        return Err(Error::validation(
            "Bucket name cannot start or end with hyphen or dot",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_object_key_valid() {
        assert!(validate_object_key("path/to/object.txt").is_ok());
        assert!(validate_object_key("simple.txt").is_ok());
        assert!(validate_object_key("a").is_ok());
    }

    #[test]
    fn test_validate_object_key_empty() {
        assert!(validate_object_key("").is_err());
    }

    #[test]
    fn test_validate_object_key_null_byte() {
        assert!(validate_object_key("path\0file").is_err());
    }

    #[test]
    fn test_validate_object_key_path_traversal() {
        assert!(validate_object_key("path/../secret").is_err());
    }

    #[test]
    fn test_validate_object_key_leading_trailing_slash() {
        assert!(validate_object_key("/path/file").is_err());
        assert!(validate_object_key("path/file/").is_err());
    }

    #[test]
    fn test_validate_object_key_too_long() {
        let long_key = "a".repeat(MAX_OBJECT_KEY_LENGTH + 1);
        assert!(validate_object_key(&long_key).is_err());
    }

    #[test]
    fn test_validate_bucket_name_valid() {
        assert!(validate_bucket_name("my-bucket").is_ok());
        assert!(validate_bucket_name("my.bucket").is_ok());
        assert!(validate_bucket_name("bucket123").is_ok());
    }

    #[test]
    fn test_validate_bucket_name_empty() {
        assert!(validate_bucket_name("").is_err());
    }

    #[test]
    fn test_validate_bucket_name_too_short() {
        assert!(validate_bucket_name("a").is_err());
        assert!(validate_bucket_name("ab").is_err());
    }

    #[test]
    fn test_validate_bucket_name_minimum_length() {
        assert!(validate_bucket_name("abc").is_ok());
    }

    #[test]
    fn test_validate_bucket_name_maximum_length() {
        let max_name = "a".repeat(MAX_BUCKET_NAME_LENGTH);
        assert!(validate_bucket_name(&max_name).is_ok());
    }

    #[test]
    fn test_validate_bucket_name_too_long() {
        let long_name = "a".repeat(MAX_BUCKET_NAME_LENGTH + 1);
        assert!(validate_bucket_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_bucket_name_invalid_chars() {
        assert!(validate_bucket_name("MyBucket").is_err());
        assert!(validate_bucket_name("bucket_name").is_err());
    }

    #[test]
    fn test_validate_bucket_name_invalid_start_end() {
        assert!(validate_bucket_name("-bucket").is_err());
        assert!(validate_bucket_name("bucket-").is_err());
        assert!(validate_bucket_name(".bucket").is_err());
        assert!(validate_bucket_name("bucket.").is_err());
    }

    #[test]
    fn test_validate_bucket_name_with_dots() {
        assert!(validate_bucket_name("my.bucket").is_ok());
        assert!(validate_bucket_name("my.test.bucket").is_ok());
    }

    #[test]
    fn test_validate_bucket_name_consecutive_hyphens() {
        assert!(validate_bucket_name("my--bucket").is_ok());
    }

    #[test]
    fn test_validate_bucket_name_starts_with_number() {
        assert!(validate_bucket_name("123bucket").is_ok());
        assert!(validate_bucket_name("1-bucket").is_ok());
    }
}
