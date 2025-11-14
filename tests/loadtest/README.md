# Load and Soak Tests

Goose-based load testing with realistic S3 workloads and system metrics collection.

## Running

```bash
# Automated load test (builds server, runs test, cleans up)
./tests/loadtest/scripts/loadtest.sh mixed

# Manual load test
cargo run --release -p save-api &
LOADTEST_SCENARIO=mixed \
cargo run --release --bin loadtest

# Soak test (long-running)
cargo run --release -p save-api &
LOADTEST_DURATION=3600 \
LOADTEST_USERS=10 \
cargo run --release --bin soaktest
```

## Scenarios

- `read-heavy` - 80% GET, 10% PUT, 10% DELETE
- `write-heavy` - 70% PUT, 20% GET, 10% DELETE
- `mixed` - 50% GET, 30% PUT, 10% DELETE, 10% LIST
- `multipart` - Large file uploads with 3-step multipart protocol

## Configuration

Edit `tests/loadtest/config.toml`:

```toml
[target]
endpoint = "http://localhost:9000"
access_key = "test-access-key"
secret_key = "test-secret-key"

[workload.users]
start = 1
max = 50
hatch_rate = 5

duration_secs = 60
```

## Output

Results are saved to `loadtest-results/`:
- `{scenario}-{timestamp}.json` - Machine-readable metrics
- `{scenario}-{timestamp}.md` - Human-readable summary
