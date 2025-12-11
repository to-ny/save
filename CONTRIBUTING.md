# Contributing

## Prerequisites

- Rust 1.85+ (edition 2024)
- clang, cmake (for RocksDB)
- Docker (for integration tests and local stack)

## Project Structure

```
crates/
├── save-api/       # HTTP server, S3 handlers
├── save-storage/   # Filesystem object storage
├── save-metadata/  # RocksDB metadata layer
├── save-common/    # Shared types, config, errors
├── save-cli/       # CLI tool
└── save-proto/     # gRPC protocol definitions

tests/
├── aws-sdk-compat/ # AWS SDK compatibility
├── aws-cli-compat/ # AWS CLI compatibility
├── concurrency/    # Multi-threaded access
├── crash-recovery/ # Failure injection and recovery
├── loadtest/       # Stress testing
└── infra/          # Terraform for remote deployments
```

## Build

```bash
make build
```

Binaries output to `target/release/`:
- `save-api` - Main server
- `save` - CLI tool

## Test

```bash
make test      # Unit tests
make clippy    # Lint
make fmt       # Format code
```

### Benchmarks

```bash
cargo bench --package save-storage
cargo bench --package save-metadata
```

### Integration Tests

Tests in `tests/` are feature-gated with different setup requirements:

| Suite | Feature | Setup |
|-------|---------|-------|
| `aws-sdk-compat` | `compat_tests` | Requires running server |
| `aws-cli-compat` | `compat_tests` | Requires running server + AWS CLI |
| `concurrency` | `concurrency_tests` | Requires running server |
| `crash-recovery` | `crash_tests` | Spawns server subprocess |
| `crash-recovery` | `cluster_tests` | Spawns multi-node cluster |
| `loadtest` | `load_tests` | Remote deployment via `tests/infra/` |

See each test crate's README for setup details.

## Run

### Native

```bash
# Uses ./save.toml or defaults
cargo run --release --bin save-api

# Custom config
SAVE_CONFIG=/path/to/config.toml cargo run --release --bin save-api
```

### Docker

```bash
make docker-build              # Build image
docker compose up -d save-api  # Run save-api only
docker compose up -d           # Full stack (Prometheus, Grafana, OpenObserve)
```

See [Docker README](./docker/README.md) for more information.
