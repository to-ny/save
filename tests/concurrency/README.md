# Concurrency Tests

Black-box HTTP tests for concurrent S3 operations using the AWS SDK for Rust.

## Test Coverage

- Simultaneous PUTs to the same key
- Concurrent GETs during PUT operations
- Parallel multipart uploads
- Mixed DELETE and PUT operations

## Running

```bash
# Start server
cargo run -p save-api

# Run tests (with feature flag)
cargo test -p concurrency-tests --features concurrency_tests
```

## Configuration

Environment variables:
- `S3_ENDPOINT` (default: http://localhost:9000)
- `AWS_ACCESS_KEY_ID` (default: test-access-key)
- `AWS_SECRET_ACCESS_KEY` (default: test-secret-key)

## Notes

Tests are gated behind the `concurrency_tests` feature flag to avoid interference with normal test runs.
Enable with `--features concurrency_tests`.
