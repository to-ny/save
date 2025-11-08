use crate::{
    ObjectStorage, StorageError, fsync_dir, fsync_file, list_temp_files, remove_if_exists,
};
use std::time::SystemTime;
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

#[tokio::test]
async fn test_remove_if_exists_existing_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    tokio::fs::write(&file_path, b"data").await.unwrap();

    let result = remove_if_exists(&file_path).await.unwrap();
    assert!(result, "Should return true when file was deleted");
    assert!(!file_path.exists(), "File should be deleted");
}

#[tokio::test]
async fn test_remove_if_exists_nonexistent_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("nonexistent.txt");

    let result = remove_if_exists(&file_path).await.unwrap();
    assert!(!result, "Should return false when file doesn't exist");
}

#[tokio::test]
async fn test_remove_if_exists_idempotent() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    tokio::fs::write(&file_path, b"data").await.unwrap();

    let result1 = remove_if_exists(&file_path).await.unwrap();
    assert!(result1, "First call should delete file");

    let result2 = remove_if_exists(&file_path).await.unwrap();
    assert!(!result2, "Second call should return false");
}

#[tokio::test]
async fn test_list_temp_files_empty_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let files = list_temp_files(temp_dir.path()).await.unwrap();
    assert_eq!(files.len(), 0, "Empty directory should return no files");
}

#[tokio::test]
async fn test_list_temp_files_nonexistent_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let nonexistent = temp_dir.path().join("does-not-exist");
    let files = list_temp_files(&nonexistent).await.unwrap();
    assert_eq!(
        files.len(),
        0,
        "Nonexistent directory should return empty list"
    );
}

#[tokio::test]
async fn test_list_temp_files_flat_structure() {
    let temp_dir = tempfile::tempdir().unwrap();

    tokio::fs::write(temp_dir.path().join("file1.txt"), b"data1")
        .await
        .unwrap();
    tokio::fs::write(temp_dir.path().join("file2.txt"), b"data2")
        .await
        .unwrap();

    let files = list_temp_files(temp_dir.path()).await.unwrap();
    assert_eq!(files.len(), 2, "Should find 2 files");

    for (path, modified) in &files {
        assert!(path.exists(), "File should exist");
        assert!(
            modified.elapsed().unwrap().as_secs() < 10,
            "File should be recently created"
        );
    }
}

#[tokio::test]
async fn test_list_temp_files_nested_structure() {
    let temp_dir = tempfile::tempdir().unwrap();

    let nested_dir = temp_dir.path().join("subdir");
    tokio::fs::create_dir(&nested_dir).await.unwrap();

    tokio::fs::write(temp_dir.path().join("file1.txt"), b"data1")
        .await
        .unwrap();
    tokio::fs::write(nested_dir.join("file2.txt"), b"data2")
        .await
        .unwrap();
    tokio::fs::write(nested_dir.join("file3.txt"), b"data3")
        .await
        .unwrap();

    let files = list_temp_files(temp_dir.path()).await.unwrap();
    assert_eq!(files.len(), 3, "Should find 3 files recursively");
}

#[tokio::test]
async fn test_list_temp_files_with_modification_time() {
    let temp_dir = tempfile::tempdir().unwrap();

    let file_path = temp_dir.path().join("test.txt");
    tokio::fs::write(&file_path, b"data").await.unwrap();

    let files = list_temp_files(temp_dir.path()).await.unwrap();
    assert_eq!(files.len(), 1);

    let (path, modified) = &files[0];
    assert_eq!(path, &file_path);

    let age = SystemTime::now().duration_since(*modified).unwrap();
    assert!(age.as_secs() < 5, "File should be very recent");
}
