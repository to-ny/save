# save-common

Shared types, errors, and configuration used across all save crates.

## Usage

```rust
use save_common::{
    config::SaveConfig,
    Bucket,
    Error, Result,
};
use std::path::Path;

// Load configuration from TOML
let config = SaveConfig::load(Path::new("save.toml"))?;

// Create a bucket
let bucket = Bucket::new("my-bucket".to_string());
```

## Configuration

Example `save.toml`:

```toml
[server]
bind_address = "127.0.0.1:9000"
max_body_size = 104857600

[storage]
data_path = "/var/lib/save/data"
metadata_path = "/var/lib/save/metadata"
max_object_size = 5368709120

[credentials]
access_key = "saveadmin"
secret_key = "savepass"
```

## Testing

```bash
cargo test -p save-common
```
