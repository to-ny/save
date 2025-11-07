# save-metadata

RocksDB-based metadata management for buckets, objects, and multipart uploads.

## System Dependencies

Building this crate requires:
- `clang` and `libclang-dev` (for RocksDB bindings)
- C++ compiler (`g++` or `clang++`)
- Standard build tools (`make`, `cmake`)

On Ubuntu/Debian:
```bash
sudo apt-get install clang libclang-dev build-essential
```

On macOS:
```bash
brew install llvm
```

## Usage

```rust
use save_metadata::{MetadataStore, ObjectMetadata};

let store = MetadataStore::new("/data/metadata").unwrap();

// Bucket operations
store.create_bucket("my-bucket").await?;
let bucket = store.get_bucket("my-bucket").await?;

// Object metadata
let metadata = ObjectMetadata::new(
    "my-bucket".to_string(),
    "file.txt".to_string(),
    1024,
    "etag".to_string(),
);
store.put_object_metadata(metadata).await?;

// Multipart uploads
let upload = store
    .initiate_multipart_upload("bucket", "key", "upload-id")
    .await?;
store.record_part("bucket", "key", "upload-id", 1, "etag".to_string(), 1024).await?;
```

## Storage Layout

Metadata is stored in RocksDB with key prefixes:
- `bkt:{bucket}` - Bucket information
- `obj:{bucket}/{key}` - Object metadata
- `mpu:{bucket}:{key}:{upload_id}` - Multipart upload state

## Testing

```bash
cargo test -p save-metadata
```
