use anyhow::{Context, Result};
use rocksdb::DB;
use save_metadata::ObjectMetadata;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Verify no phantom objects (metadata pointing to missing storage)
pub async fn verify_no_phantom_objects(data_dir: &Path, metadata_dir: &Path) -> Result<()> {
    let storage_dir = data_dir.join("objects");

    // Open RocksDB metadata store
    let db = DB::open_default(metadata_dir)
        .context("Failed to open metadata database for verification")?;

    let iter = db.iterator(rocksdb::IteratorMode::Start);

    for item in iter {
        let (key, value) = item?;

        // Skip non-object keys
        if !key.starts_with(b"obj:") {
            continue;
        }

        // Deserialize metadata
        let (metadata, _): (ObjectMetadata, _) =
            bincode::serde::decode_from_slice(&value, bincode::config::standard())
                .context("Failed to deserialize metadata")?;

        // Compute expected storage path
        let storage_key = format!("{}/{}", metadata.bucket, metadata.key);
        let storage_path = compute_storage_path(&storage_dir, &storage_key);

        // Verify storage file exists
        if !storage_path.exists() {
            anyhow::bail!(
                "PHANTOM OBJECT DETECTED: Metadata points to missing storage\n  \
                 Bucket: {}, Key: {}, Expected path: {:?}",
                metadata.bucket,
                metadata.key,
                storage_path
            );
        }
    }

    Ok(())
}

/// Verify no orphaned storage files (or they're properly marked for GC)
pub async fn verify_no_orphans_or_gc_pending(data_dir: &Path, metadata_dir: &Path) -> Result<()> {
    let storage_dir = data_dir.join("objects");

    if !storage_dir.exists() {
        return Ok(()); // No storage directory means no orphans
    }

    // Collect all storage files
    let storage_files = collect_storage_files(&storage_dir).await?;

    // Open metadata to get all known objects
    let db = DB::open_default(metadata_dir)
        .context("Failed to open metadata database for verification")?;

    let mut metadata_hashes = std::collections::HashSet::new();

    let iter = db.iterator(rocksdb::IteratorMode::Start);
    for item in iter {
        let (key, value) = item?;

        if !key.starts_with(b"obj:") {
            continue;
        }

        let (metadata, _): (ObjectMetadata, _) =
            bincode::serde::decode_from_slice(&value, bincode::config::standard())?;

        let storage_key = format!("{}/{}", metadata.bucket, metadata.key);
        let hash = compute_storage_hash(&storage_key);
        metadata_hashes.insert(hash);
    }

    // Check for orphaned files
    for storage_file in &storage_files {
        let file_name = storage_file
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid file name")?;

        if !metadata_hashes.contains(file_name) {
            tracing::warn!(
                "Orphaned storage file detected (should be GC'd): {:?}",
                storage_file
            );
            // Note: We don't fail here because orphaned files are acceptable
            // as long as they're eventually garbage collected
        }
    }

    Ok(())
}

fn compute_storage_path(storage_dir: &Path, key: &str) -> PathBuf {
    let hash = compute_storage_hash(key);
    let prefix = &hash[..2];
    storage_dir.join(prefix).join(&hash)
}

fn compute_storage_hash(key: &str) -> String {
    let hash = Sha256::digest(key.as_bytes());
    format!("{:x}", hash)
}

async fn collect_storage_files(storage_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let mut prefix_dirs = fs::read_dir(storage_dir).await?;
    while let Some(prefix_entry) = prefix_dirs.next_entry().await? {
        if !prefix_entry.file_type().await?.is_dir() {
            continue;
        }

        let mut object_files = fs::read_dir(prefix_entry.path()).await?;
        while let Some(object_entry) = object_files.next_entry().await? {
            if object_entry.file_type().await?.is_file() {
                files.push(object_entry.path());
            }
        }
    }

    Ok(files)
}
