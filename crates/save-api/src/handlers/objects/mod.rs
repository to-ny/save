mod delete;
mod get;
mod put;

pub use delete::delete_object;
pub use get::get_object;
pub use put::put_object;

pub fn storage_key(bucket: &str, key: &str) -> String {
    format!("{}/{}", bucket, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_key_format() {
        assert_eq!(storage_key("bucket", "key"), "bucket/key");
        assert_eq!(storage_key("my-bucket", "path/to/object.txt"), "my-bucket/path/to/object.txt");
    }
}
