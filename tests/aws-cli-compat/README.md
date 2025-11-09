# AWS CLI Compatibility Tests

End-to-end tests using the official AWS CLI to validate S3 API compatibility.

## Prerequisites

- AWS CLI v2 installed: `aws --version`

## Running

```bash
# Start server
cargo run -p save-api

# Run tests (with feature flag)
cargo test -p aws-cli-compat --features compat_tests
```

## Configuration

Environment variables:
- `S3_ENDPOINT` (default: http://localhost:9000)
- `AWS_ACCESS_KEY_ID` (default: test-access-key)
- `AWS_SECRET_ACCESS_KEY` (default: test-secret-key)

## Notes

Tests are gated behind the `compat_tests` feature flag to avoid interference with normal test runs.
Enable with `--features compat_tests`.
