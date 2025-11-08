use crate::{ObjectStorage, StorageError, fsync_dir, fsync_file};
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

#[tokio::test]
async fn test_fsync_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test_file.txt");

    tokio::fs::write(&file_path, b"test data").await.unwrap();

    let result = fsync_file(&file_path).await;
    assert!(result.is_ok(), "fsync_file should succeed");
}

#[tokio::test]
async fn test_fsync_dir() {
    let temp_dir = tempfile::tempdir().unwrap();

    let result = fsync_dir(temp_dir.path()).await;
    assert!(result.is_ok(), "fsync_dir should succeed");
}

#[tokio::test]
async fn test_write_temp_object_and_commit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let key = "bucket/test-object";
    let data = b"atomic test data";

    let temp_object = storage.write_temp_object(key, &data[..]).await.unwrap();

    assert!(temp_object.temp_path().exists(), "Temp file should exist");

    storage.commit_object(temp_object).await.unwrap();

    let mut file = storage.get_object(key).await.unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();
    assert_eq!(contents, data);
}

#[tokio::test]
async fn test_temp_object_auto_cleanup_on_drop() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let key = "bucket/test-auto-cleanup";
    let data = b"auto cleanup test";

    let temp_path = {
        let temp_object = storage.write_temp_object(key, &data[..]).await.unwrap();
        let path = temp_object.temp_path().to_path_buf();
        assert!(path.exists(), "Temp file should exist");
        path
    };

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert!(
        !temp_path.exists(),
        "Temp file should be auto-cleaned up on drop"
    );
}

#[tokio::test]
async fn test_temp_object_no_cleanup_after_commit() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = ObjectStorage::new(temp_dir.path()).await.unwrap();

    let key = "bucket/test-no-cleanup";
    let data = b"no cleanup after commit";

    let temp_object = storage.write_temp_object(key, &data[..]).await.unwrap();
    storage.commit_object(temp_object).await.unwrap();

    let mut file = storage.get_object(key).await.unwrap();
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).await.unwrap();
    assert_eq!(contents, data);
}
