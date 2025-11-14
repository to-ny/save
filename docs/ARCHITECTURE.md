# Architecture

Distributed object storage system inspired by MinIO and S3. Currently Phase 1 (single-node); will evolve into distributed, erasure-coded, self-healing cluster.

## System Overview

### Core Principles
- S3-compatible API
- Layered modularity (separate crates)
- Progressive expansion (single-node → distributed)
- Rust safety with async I/O (tokio, tracing, serde)

### Data Flow

```
[S3 Client]
    ↓
[API Service - HTTP (axum)]
    ↓
[Storage Layer]
 ├─ Metadata (RocksDB)
 └─ Object Data (local FS)
```

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `save-api` | S3-compatible REST API (PUT/GET/DELETE, multipart, auth) |
| `save-storage` | Object persistence, streaming I/O, filesystem layout |
| `save-metadata` | RocksDB operations for buckets, objects, versions |
| `save-common` | Shared types: errors, config, checksums, locks |
| `save-cli` | CLI tool for administration and testing |

## Phase 1: Single-Node Architecture

### Components
- **HTTP Frontend**: axum-based S3 handlers
- **Auth Layer**: SigV4 validation with static credentials
- **Metadata**: RocksDB for bucket/object metadata
- **Storage**: Content-addressed filesystem layout (SHA256)
- **Multipart**: State machine for multi-part uploads
- **GC Worker**: Periodic cleanup of orphaned temp files
- **Observability**: `/metrics`, `/health`, structured logging

### Key Guarantees
- Strong consistency (single node)
- Atomic writes via temp file + atomic rename
- Crash recovery via RocksDB durability + fsync

## Data Model

### Metadata (RocksDB)
```
bkt:{name}               → bucket record
obj:{bucket}/{key}       → object metadata
mpu:{bucket}/{key}:{id}  → multipart upload state
```

Values: Binary (bincode serialization)

### Filesystem Layout
```
data/
├─ objects/
│  └─ {hash[0:2]}/{hash[2:4]}/{hash}/  # Content-addressed
├─ temp/
│  └─ parts/                            # Multipart temp files
└─ rocksdb/                             # Metadata store
```

## Non-Functional Design

| Concern | Strategy |
|---------|----------|
| **Performance** | Streamed I/O, zero-copy when possible |
| **Reliability** | Atomic operations, crash recovery |
| **Security** | TLS, SigV4 auth, static credentials (Phase 1) |
| **Extensibility** | Modular crates for phased growth |
| **Observability** | Structured logging, Prometheus metrics |

## Testing Strategy

- **Unit tests**: Inside each crate
- **Integration tests**: `tests/` directory
  - AWS SDK compatibility (`tests/aws-sdk-compat/`)
  - AWS CLI compatibility (`tests/aws-cli-compat/`)
  - Concurrency (`tests/concurrency/`)
  - Crash recovery (`tests/crash-recovery/`)
  - Load and soak tests (`tests/loadtest/`)
- **Benchmarks**: In each crate's `benches/` directory
  - `save-storage/benches/` - Storage layer (PUT/GET/DELETE operations)
  - `save-metadata/benches/` - RocksDB operations (buckets, objects, listing)
  - `save-common/benches/` - Crypto (SHA256, HMAC) and validation (locking)

See individual test READMEs and [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) for details.

## Future Evolution

Phase 2-4 roadmap available in [ROADMAP.md](ROADMAP.md):
- **Phase 2**: Replication, cluster coordination (Raft/etcd)
- **Phase 3**: Erasure coding, healing, rebalancing
- **Phase 4**: Multi-tenant IAM, metrics, management API
