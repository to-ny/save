use crate::{ObjectStorage, StorageError};
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn test_put_get_delete_object() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let key = "test-bucket/test-object";
    let data = b"hello world";

    storage.put_object(key, &data[..]).await.unwrap();

    let mut file = storage.get_object(key).await.unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();
    assert_eq!(contents, data);

    storage.delete_object(key).await.unwrap();

    let result = storage.get_object(key).await;
    assert!(matches!(result, Err(StorageError::NotFound(_))));
}

#[tokio::test]
async fn test_get_nonexistent_object() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let result = storage.get_object("nonexistent").await;
    assert!(matches!(result, Err(StorageError::NotFound(_))));
}

#[tokio::test]
async fn test_delete_nonexistent_object() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let result = storage.delete_object("nonexistent").await;
    assert!(matches!(result, Err(StorageError::NotFound(_))));
}

#[tokio::test]
async fn test_large_object_streaming() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let key = "bucket/large-object";
    let data = vec![42u8; 1024 * 1024];

    storage.put_object(key, &data[..]).await.unwrap();

    let mut file = storage.get_object(key).await.unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();

    assert_eq!(contents.len(), data.len());
    assert_eq!(contents, data);

    storage.delete_object(key).await.unwrap();
}

#[tokio::test]
async fn test_invalid_key_rejected() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let invalid_keys = vec![
        "../etc/passwd",
        "bucket/../../../etc/passwd",
        "",
        "/bucket/key",
        "bucket/key/",
        "bucket/key\0",
    ];

    for key in invalid_keys {
        let result = storage.put_object(key, &b"data"[..]).await;
        assert!(
            matches!(result, Err(StorageError::InvalidKey(_))),
            "Expected InvalidKey error for key: {}",
            key
        );

        let result = storage.get_object(key).await;
        assert!(
            matches!(result, Err(StorageError::InvalidKey(_))),
            "Expected InvalidKey error for key: {}",
            key
        );
    }
}
