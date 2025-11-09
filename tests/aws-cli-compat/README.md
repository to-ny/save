# AWS CLI Compatibility Tests

End-to-end tests using the official AWS CLI to validate S3 API compatibility.

## Prerequisites

- AWS CLI v2 installed: `aws --version`

## Running

```bash
# Start server
cargo run -p save-api

# Run tests
cargo test -p aws-cli-compat
```
