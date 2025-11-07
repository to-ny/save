# save-api

Axum-based HTTP server implementing the S3-compatible REST API.

## Usage

```bash
# Run server (listens on port 9000)
cargo run --bin save-api

# Run tests
cargo test

# Configure logging
RUST_LOG=save_api=debug cargo run --bin save-api
```

## Current Endpoints

- `GET /health` - Health check, returns `{"status": "ok"}`
