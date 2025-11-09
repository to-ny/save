//! Object operation compatibility tests.

mod common;

use common::{cleanup_bucket, create_client, ensure_bucket, unique_bucket_name};
use anyhow::{Context, Result};
use tracing::info;

#[tokio::test]
async fn test_put_and_get_object() -> Result<()> {
    let client = create_client().await;
    let bucket = unique_bucket_name("sdk-compat-putget");
    let key = "test-file.txt";
    let content = b"Hello, World! This is a test file.";

    // Setup
    ensure_bucket(&client, &bucket).await?;

    // Put object
    info!("Uploading object: {}/{}", bucket, key);
    let put_response = client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(content.to_vec().into())
        .send()
        .await
        .context("Failed to put object")?;

    let put_etag = put_response.e_tag.clone();
    assert!(put_etag.is_some(), "PUT should return an ETag");

    // Get object
    info!("Retrieving object: {}/{}", bucket, key);
    let get_response = client
        .get_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to get object")?;

    // Verify metadata
    assert_eq!(
        get_response.content_length,
        Some(content.len() as i64),
        "Content length should match"
    );
    assert_eq!(
        get_response.e_tag,
        put_etag,
        "ETag should match between PUT and GET"
    );

    // Verify content
    let body_bytes = get_response
        .body
        .collect()
        .await
        .context("Failed to read response body")?
        .into_bytes();

    assert_eq!(
        body_bytes.as_ref(),
        content,
        "Retrieved content should match uploaded content"
    );

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_head_object() -> Result<()> {
    let client = create_client().await;
    let bucket = unique_bucket_name("sdk-compat-head");
    let key = "test-file.txt";
    let content = b"Test content for HEAD operation";

    // Setup
    ensure_bucket(&client, &bucket).await?;

    // Put object
    info!("Uploading object: {}/{}", bucket, key);
    let put_response = client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(content.to_vec().into())
        .send()
        .await
        .context("Failed to put object")?;

    let put_etag = put_response.e_tag.clone();

    // Head object
    info!("HEAD object: {}/{}", bucket, key);
    let head_response = client
        .head_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to HEAD object")?;

    // Verify metadata
    assert_eq!(
        head_response.content_length,
        Some(content.len() as i64),
        "HEAD content length should match"
    );
    assert_eq!(
        head_response.e_tag, put_etag,
        "HEAD ETag should match PUT ETag"
    );

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_list_objects() -> Result<()> {
    let client = create_client().await;
    let bucket = unique_bucket_name("sdk-compat-list");

    // Setup
    ensure_bucket(&client, &bucket).await?;

    // Upload multiple objects with different prefixes
    let objects = vec![
        ("docs/readme.md", "README content"),
        ("docs/guide.md", "Guide content"),
        ("data/file1.csv", "CSV data 1"),
        ("data/file2.csv", "CSV data 2"),
        ("root.txt", "Root level file"),
    ];

    info!("Uploading {} objects", objects.len());
    for (key, content) in &objects {
        client
            .put_object()
            .bucket(&bucket)
            .key(*key)
            .body(content.as_bytes().to_vec().into())
            .send()
            .await
            .context(format!("Failed to upload {}", key))?;
    }

    // List all objects
    info!("Listing all objects in bucket");
    let list_response = client
        .list_objects_v2()
        .bucket(&bucket)
        .send()
        .await
        .context("Failed to list objects")?;

    let all_keys: Vec<String> = list_response
        .contents
        .unwrap_or_default()
        .iter()
        .filter_map(|obj| obj.key.clone())
        .collect();

    assert_eq!(
        all_keys.len(),
        objects.len(),
        "Should list all uploaded objects"
    );

    // List with prefix
    info!("Listing objects with prefix 'docs/'");
    let list_response = client
        .list_objects_v2()
        .bucket(&bucket)
        .prefix("docs/")
        .send()
        .await
        .context("Failed to list objects with prefix")?;

    let docs_keys: Vec<String> = list_response
        .contents
        .unwrap_or_default()
        .iter()
        .filter_map(|obj| obj.key.clone())
        .collect();

    assert_eq!(
        docs_keys.len(),
        2,
        "Should list only objects with 'docs/' prefix"
    );
    assert!(docs_keys.iter().all(|k| k.starts_with("docs/")));

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_delete_object() -> Result<()> {
    let client = create_client().await;
    let bucket = unique_bucket_name("sdk-compat-delete");
    let key = "file-to-delete.txt";

    // Setup
    ensure_bucket(&client, &bucket).await?;

    // Upload object
    info!("Uploading object: {}/{}", bucket, key);
    client
        .put_object()
        .bucket(&bucket)
        .key(key)
        .body(b"This will be deleted".to_vec().into())
        .send()
        .await
        .context("Failed to put object")?;

    // Verify object exists
    info!("Verifying object exists");
    let list_response = client
        .list_objects_v2()
        .bucket(&bucket)
        .send()
        .await
        .context("Failed to list objects")?;

    let keys: Vec<String> = list_response
        .contents
        .unwrap_or_default()
        .iter()
        .filter_map(|obj| obj.key.clone())
        .collect();

    assert!(keys.contains(&key.to_string()), "Object should exist");

    // Delete object
    info!("Deleting object: {}/{}", bucket, key);
    client
        .delete_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("Failed to delete object")?;

    // Verify object is gone
    info!("Verifying object was deleted");
    let list_response = client
        .list_objects_v2()
        .bucket(&bucket)
        .send()
        .await
        .context("Failed to list objects after delete")?;

    let keys: Vec<String> = list_response
        .contents
        .unwrap_or_default()
        .iter()
        .filter_map(|obj| obj.key.clone())
        .collect();

    assert!(
        !keys.contains(&key.to_string()),
        "Object should no longer exist"
    );

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_delete_object_idempotent() -> Result<()> {
    let client = create_client().await;
    let bucket = unique_bucket_name("sdk-compat-delete-idemp");
    let key = "nonexistent.txt";

    // Setup
    ensure_bucket(&client, &bucket).await?;

    // Delete non-existent object (should succeed)
    info!("Deleting non-existent object: {}/{}", bucket, key);
    client
        .delete_object()
        .bucket(&bucket)
        .key(key)
        .send()
        .await
        .context("DELETE should be idempotent")?;

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}

#[tokio::test]
async fn test_list_objects_with_pagination() -> Result<()> {
    let client = create_client().await;
    let bucket = unique_bucket_name("sdk-compat-pagination");

    // Setup
    ensure_bucket(&client, &bucket).await?;

    // Upload more than 10 objects to test pagination
    info!("Uploading 15 objects for pagination test");
    for i in 0..15 {
        let key = format!("file-{:02}.txt", i);
        client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .body(format!("Content {}", i).into_bytes().into())
            .send()
            .await
            .context(format!("Failed to upload {}", key))?;
    }

    // List with max_keys=5
    info!("Listing objects with max_keys=5");
    let list_response = client
        .list_objects_v2()
        .bucket(&bucket)
        .max_keys(5)
        .send()
        .await
        .context("Failed to list objects with pagination")?;

    let first_page_keys: Vec<String> = list_response
        .contents
        .unwrap_or_default()
        .iter()
        .filter_map(|obj| obj.key.clone())
        .collect();

    assert_eq!(
        first_page_keys.len(),
        5,
        "First page should have 5 objects"
    );
    assert_eq!(
        list_response.is_truncated,
        Some(true),
        "Response should indicate more results available"
    );

    // Get next page using continuation token
    if let Some(token) = list_response.next_continuation_token {
        info!("Fetching next page with continuation token");
        let next_response = client
            .list_objects_v2()
            .bucket(&bucket)
            .max_keys(5)
            .continuation_token(token)
            .send()
            .await
            .context("Failed to list next page")?;

        let next_page_keys: Vec<String> = next_response
            .contents
            .unwrap_or_default()
            .iter()
            .filter_map(|obj| obj.key.clone())
            .collect();

        assert_eq!(
            next_page_keys.len(),
            5,
            "Second page should have 5 objects"
        );
    }

    // Cleanup
    cleanup_bucket(&client, &bucket).await?;

    Ok(())
}
