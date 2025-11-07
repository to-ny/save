# save-storage

Object persistence layer for local filesystem storage.

## Usage

```rust
use save_storage::ObjectStorage;

let storage = ObjectStorage::new("/data");

// Write object
storage.put_object("bucket/key", data_reader).await?;

// Read object
let file = storage.get_object("bucket/key").await?;

// Delete object
storage.delete_object("bucket/key").await?;
```

## Storage Layout

Objects are stored using content-addressable paths:
- Path: `objects/<2-char-prefix>/<sha256-hash>`
- Temp files: `temp/<sha256-hash>.tmp`
- Atomic writes via temp file + rename

## Testing

```bash
cargo test -p save-storage
```
