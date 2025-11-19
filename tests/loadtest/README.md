# Load Tests

Custom S3 benchmark framework with server-side storage timing and latency breakdown.

## Running

```bash
# Start server
cargo run --release -p save-api

# Quick smoke test
cargo test -p save-loadtest --features load_tests test_quick_smoke

# Workload tests
cargo test -p save-loadtest --features load_tests test_mixed_workload
cargo test -p save-loadtest --features load_tests test_read_heavy_workload
cargo test -p save-loadtest --features load_tests test_write_heavy_workload

# Soak tests
cargo test -p save-loadtest --features load_tests test_soak_short  # 10 min
LOADTEST_DURATION=3600 cargo test -p save-loadtest --features load_tests test_soak  # 1 hour
```

Results saved to `loadtest-results/` as Markdown.
