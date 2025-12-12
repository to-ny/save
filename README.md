# Save

S3-compatible object storage written in Rust.

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

## Features

- S3-compatible REST API (PUT, GET, DELETE, multipart uploads)
- RocksDB metadata with content-addressed filesystem storage
- Raft-based cluster coordination
- SigV4 authentication
- Prometheus metrics and health endpoints

## Quick Start

```bash
# Create config file
cat > save.toml << 'EOF'
[server]
bind_address = "0.0.0.0:9000"

[storage]
data_path = "/var/lib/save/data"
metadata_path = "/var/lib/save/metadata"

[credentials]
access_key = "test-access-key"
secret_key = "test-secret-key"
EOF

# Run container
docker run -d \
  --name save \
  -p 9000:9000 \
  -v $(pwd)/save.toml:/app/save.toml:ro \
  -v save-data:/var/lib/save/data \
  -v save-metadata:/var/lib/save/metadata \
  ghcr.io/to-ny/save:latest

# Verify
curl http://localhost:9000/health
```

## Usage with AWS CLI

```bash
export AWS_ACCESS_KEY_ID=test-access-key
export AWS_SECRET_ACCESS_KEY=test-secret-key

aws --endpoint-url http://localhost:9000 s3 mb s3://my-bucket
aws --endpoint-url http://localhost:9000 s3 cp file.txt s3://my-bucket/
aws --endpoint-url http://localhost:9000 s3 ls s3://my-bucket/
```

## Kubernetes Deployment

Deploy a 3-node cluster using Helm:

```bash
helm install save ./charts/save -f charts/save/values-development.yaml
```

For production, create a credentials secret first:

```bash
kubectl create secret generic save-credentials \
  --from-literal=access-key=YOUR_ACCESS_KEY \
  --from-literal=secret-key=YOUR_SECRET_KEY

helm install save ./charts/save \
  -f charts/save/values-production.yaml \
  --set auth.existingSecret=save-credentials
```

See [charts/save/README.md](charts/save/README.md) for full documentation.

## Configuration

Mount your config file to `/app/save.toml` in the container.

See [`docker/save/save.toml`](docker/save/save.toml) for all available options.

### Environment Variables

| Variable | Description |
|----------|-------------|
| `SAVE_CONFIG` | Config file path (default: `/app/save.toml`) |
| `LOG_FORMAT` | Set to `json` for JSON logging |
| `RUST_LOG` | Log level filter (e.g., `save_api=debug`) |

## Documentation

- [Architecture](docs/ARCHITECTURE.md) - System design and components
- [Roadmap](docs/ROADMAP.md) - Development phases

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for build instructions and development workflow.

## License

Apache 2.0
