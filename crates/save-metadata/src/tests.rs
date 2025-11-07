use crate::{MetadataStore, ObjectMetadata};
use tempfile::TempDir;

fn create_test_store() -> (MetadataStore, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = MetadataStore::new(temp_dir.path()).unwrap();
    (store, temp_dir)
}

#[tokio::test]
async fn test_bucket_lifecycle() {
    let (store, _temp_dir) = create_test_store();

    let bucket = store.create_bucket("test-bucket").await.unwrap();
    assert_eq!(bucket.name, "test-bucket");

    let fetched = store.get_bucket("test-bucket").await.unwrap();
    assert_eq!(fetched.name, "test-bucket");

    store.delete_bucket("test-bucket").await.unwrap();

    let result = store.get_bucket("test-bucket").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_object_metadata_lifecycle() {
    let (store, _temp_dir) = create_test_store();

    let metadata = ObjectMetadata::new(
        "bucket".to_string(),
        "key".to_string(),
        1024,
        "etag".to_string(),
    );

    store.put_object_metadata(metadata.clone()).await.unwrap();

    let fetched = store.get_object_metadata("bucket", "key").await.unwrap();
    assert_eq!(fetched.size, 1024);
    assert_eq!(fetched.etag, "etag");

    store.delete_object_metadata("bucket", "key").await.unwrap();

    let result = store.get_object_metadata("bucket", "key").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multipart_upload_lifecycle() {
    let (store, _temp_dir) = create_test_store();

    let upload = store
        .initiate_multipart_upload("bucket", "key", "upload123")
        .await
        .unwrap();
    assert_eq!(upload.upload_id, "upload123");

    store
        .record_part("bucket", "key", "upload123", 1, "etag1".to_string(), 1024)
        .await
        .unwrap();

    store
        .record_part("bucket", "key", "upload123", 2, "etag2".to_string(), 2048)
        .await
        .unwrap();

    let upload = store
        .get_multipart_upload("bucket", "key", "upload123")
        .await
        .unwrap();
    assert_eq!(upload.parts.len(), 2);

    let completed = store
        .complete_multipart_upload("bucket", "key", "upload123")
        .await
        .unwrap();
    assert_eq!(completed.parts.len(), 2);

    let result = store
        .get_multipart_upload("bucket", "key", "upload123")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_buckets() {
    let (store, _temp_dir) = create_test_store();

    store.create_bucket("bucket1").await.unwrap();
    store.create_bucket("bucket2").await.unwrap();

    let buckets = store.list_buckets().await.unwrap();
    assert_eq!(buckets.len(), 2);
}

#[tokio::test]
async fn test_list_objects() {
    let (store, _temp_dir) = create_test_store();

    let obj1 = ObjectMetadata::new(
        "bucket".to_string(),
        "file1.txt".to_string(),
        100,
        "etag1".to_string(),
    );
    let obj2 = ObjectMetadata::new(
        "bucket".to_string(),
        "file2.txt".to_string(),
        200,
        "etag2".to_string(),
    );

    store.put_object_metadata(obj1).await.unwrap();
    store.put_object_metadata(obj2).await.unwrap();

    let objects = store.list_objects("bucket", None).await.unwrap();
    assert_eq!(objects.len(), 2);
}
