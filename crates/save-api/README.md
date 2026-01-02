# save-api

Axum-based HTTP server implementing the S3-compatible REST API.

## Usage

```bash
# Run server (default port 9000)
cargo run --bin save-api

# With custom config
SAVE_CONFIG=./my-config.toml cargo run --bin save-api

# Configure logging
RUST_LOG=save_api=debug cargo run --bin save-api

# Run tests
cargo test -p save-api
```

## Endpoints

### Health & Metrics

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Basic health check |
| GET | `/health/ready` | Readiness probe with deep health checks |
| GET | `/metrics` | Prometheus metrics |

### Cluster Management

| Method | Path | Description |
|--------|------|-------------|
| GET | `/cluster/status` | Cluster status (leader, members, health) |
| POST | `/cluster/initialize` | Initialize Raft cluster |
| POST | `/cluster/members` | Add learner node |
| POST | `/cluster/members/promote` | Promote learners to voters |
| DELETE | `/cluster/members/{node_id}` | Remove node from cluster |

### Bucket Operations

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | List all buckets |
| PUT | `/{bucket}` | Create bucket |
| DELETE | `/{bucket}` | Delete bucket (must be empty) |
| GET | `/{bucket}` | List objects in bucket |
| GET | `/{bucket}?uploads` | List multipart uploads |

### Object Operations

| Method | Path | Description |
|--------|------|-------------|
| PUT | `/{bucket}/{key}` | Upload object |
| GET | `/{bucket}/{key}` | Download object |
| HEAD | `/{bucket}/{key}` | Get object metadata |
| DELETE | `/{bucket}/{key}` | Delete object |

### Multipart Upload

| Method | Path | Description |
|--------|------|-------------|
| POST | `/{bucket}/{key}?uploads` | Initiate multipart upload |
| PUT | `/{bucket}/{key}?partNumber=N&uploadId=ID` | Upload part |
| POST | `/{bucket}/{key}?uploadId=ID` | Complete multipart upload |
| DELETE | `/{bucket}/{key}?uploadId=ID` | Abort multipart upload |

## Authentication

Supports AWS Signature Version 4 (SigV4) authentication. Configure credentials in `save.toml`:

```toml
[auth]
access_key = "your-access-key"
secret_key = "your-secret-key"
```

## Configuration

See `save.toml.example` for all configuration options including:
- Server settings (bind address, timeouts)
- Storage paths and fsync modes
- Cluster configuration (node ID, peers, replication)
- Metadata tuning (RocksDB settings)
