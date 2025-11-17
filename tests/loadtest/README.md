# Load and Soak Tests

Goose-based load testing with realistic S3 workloads (mixed, read-heavy, write-heavy, multipart).

## Prerequisites

```bash
# Start server
cargo run --release -p save-api

# Create test bucket
cargo run --release -p save-cli -- \
  --endpoint http://localhost:9000 \
  --access-key test-access-key \
  --secret-key test-secret-key \
  bucket create loadtest
```

## Running

```bash
# All tests
cargo test -p save-loadtest --features load_tests

# Quick smoke test
cargo test -p save-loadtest --features load_tests test_quick_smoke

# Specific scenario
cargo test -p save-loadtest --features load_tests test_mixed_workload

# Soak test (1 hour)
cargo test -p save-loadtest --features load_tests test_soak
```

## Configuration

Environment variables:
- `LOADTEST_CONFIG` - Config file path (default: `tests/loadtest/config.toml`)
- `LOADTEST_DURATION` - Duration in seconds (soak tests)
- `LOADTEST_USERS` - Concurrent users

## Notes

Tests are gated behind the `load_tests` feature flag. Results saved to `loadtest-results/`.
