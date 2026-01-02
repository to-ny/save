# Save

**Kubernetes-native S3-compatible object storage written in Rust.**

[![CI](https://github.com/to-ny/save/actions/workflows/ci.yml/badge.svg)](https://github.com/to-ny/save/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/to-ny/save/branch/main/graph/badge.svg)](https://codecov.io/gh/to-ny/save)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

## Project Status

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | Single-node S3-compatible storage | Complete |
| Phase 2 | Raft consensus + quorum replication | In Progress |
| Phase 3 | Erasure coding, healing, rebalancing | Planned |
| Phase 4 | Multi-tenant IAM, operator tooling | Planned |

**Current capabilities**: Full S3 API (buckets, objects, multipart), AWS SigV4 auth, 3-node Raft cluster, quorum writes, Prometheus metrics, Helm deployment.

## Features

- **Kubernetes-native**: Operator-managed lifecycle (bootstrap, scaling, healing)
- **S3-compatible**: REST API works with existing S3 tools and SDKs
- **Distributed**: Raft consensus for consistent metadata, quorum writes for durability
- **Observable**: Prometheus metrics, structured logging, health endpoints

## Quick Start

Deploy a 3-node cluster using Helm:

```bash
helm install save ./charts/save \
  --set replicaCount=3 \
  --set auth.accessKey=my-access-key \
  --set auth.secretKey=my-secret-key
```

For production, use an existing secret:

```bash
kubectl create secret generic save-credentials \
  --from-literal=access-key=YOUR_ACCESS_KEY \
  --from-literal=secret-key=YOUR_SECRET_KEY

helm install save ./charts/save \
  --set auth.existingSecret=save-credentials
```

See [charts/save/README.md](charts/save/README.md) for full Helm documentation.

## Usage with AWS CLI

```bash
export AWS_ACCESS_KEY_ID=my-access-key
export AWS_SECRET_ACCESS_KEY=my-secret-key

aws --endpoint-url http://save.default.svc:9000 s3 mb s3://my-bucket
aws --endpoint-url http://save.default.svc:9000 s3 cp file.txt s3://my-bucket/
aws --endpoint-url http://save.default.svc:9000 s3 ls s3://my-bucket/
```

## Configuration

See [`save.toml.example`](save.toml.example) for all available options.

| Variable | Description |
|----------|-------------|
| `SAVE_CONFIG` | Config file path (default: `/app/save.toml`) |
| `LOG_FORMAT` | Set to `json` for JSON logging |
| `RUST_LOG` | Log level filter (e.g., `save_api=debug`) |

## Performance

Benchmark infrastructure exists but results are not yet published. Run locally:

```bash
cargo bench                           # Micro-benchmarks (crypto, validation, storage)
cd tests/loadtest && cargo run        # Load tests with concurrent S3 workloads
```

<!-- TODO: Run benchmarks on standardized hardware and publish results -->

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions and development workflow.

## License

Apache 2.0
