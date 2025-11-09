# AWS SDK S3 Compatibility Tests

End-to-end tests using the official AWS SDK for Rust to validate S3 API compatibility.

## Running

```bash
# Start server
cargo run -p save-api

# Run tests
cargo test -p aws-sdk-compat
```

## Configuration

Environment variables:
- `SAVE_ENDPOINT` (default: http://localhost:9000)
- `SAVE_ACCESS_KEY` (default: saveadmin)
- `SAVE_SECRET_KEY` (default: saveadmin)
