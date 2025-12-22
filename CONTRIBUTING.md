# Contributing

## Prerequisites

- Rust 1.85+ (edition 2024)
- clang, cmake (for RocksDB)
- Docker
- kubectl
- Helm 3.x
- kind or minikube (for local Kubernetes)

## Documentation

- [Architecture](docs/ARCHITECTURE.md) - System design and components
- [Roadmap](docs/ROADMAP.md) - Development phases and milestones
- [ADRs](docs/adrs/) - Architecture decision records

## Project Structure

```
crates/
├── save-api/       # HTTP server, S3 handlers
├── save-storage/   # Filesystem object storage
├── save-metadata/  # RocksDB metadata layer
├── save-common/    # Shared types, config, errors
├── save-cli/       # CLI tool
└── save-proto/     # gRPC protocol definitions

charts/
├── save/           # Helm chart for Kubernetes (production)
└── save-dev/       # Development chart with observability stack

tests/
├── aws-sdk-compat/ # AWS SDK compatibility
├── aws-cli-compat/ # AWS CLI compatibility
├── concurrency/    # Multi-threaded access
├── crash-recovery/ # Failure injection and recovery
├── loadtest/       # Stress testing
└── infra/          # Kubernetes-based load testing infrastructure
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

### Local Development (single node)

For quick iteration without Kubernetes:

```bash
# Uses ./save.toml or defaults
cargo run --release --bin save-api

# Custom config
SAVE_CONFIG=/path/to/config.toml cargo run --release --bin save-api
```

### Kubernetes (recommended)

The primary deployment target is Kubernetes. For local development with a full cluster:

```bash
# Create local cluster
kind create cluster --name save-dev

# Build and load image
docker build -t save:latest .
kind load docker-image save:latest --name save-dev

# Deploy
helm install save ./charts/save \
  --set image.repository=save \
  --set image.tag=latest \
  --set image.pullPolicy=Never
```

```bash
make helm-lint                 # Validate chart
make helm-template             # Render templates locally
```

See [Helm Chart README](./charts/save/README.md) for full documentation.

### Development with Observability

For development with full observability stack (Prometheus, Grafana, OpenObserve, Vector):

```bash
cd charts/save-dev
helm dependency update
helm install save-dev . -n save-dev --create-namespace

# Access services
kubectl port-forward svc/save-dev-grafana 3000:3000 -n save-dev      # Dashboards
kubectl port-forward svc/save-dev-openobserve 5080:5080 -n save-dev  # Logs
kubectl port-forward svc/save-dev-save 9000:9000 -n save-dev         # S3 API
```

See [Development Chart README](./charts/save-dev/README.md) for more details.
