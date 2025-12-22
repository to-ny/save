# Architecture

Distributed object storage system inspired by MinIO and S3. Currently Phase 2 (distributed replication); will evolve into erasure-coded, self-healing cluster.

---

## System Overview

### Core Principles
- S3-compatible API
- Layered modularity (separate crates)
- Progressive expansion (single-node → distributed)
- Rust safety with async I/O (tokio, tracing, serde)

### Crate Layout

| Crate | Purpose |
|-------|---------|
| `save-api` | S3-compatible REST API (PUT/GET/DELETE, multipart, auth) |
| `save-storage` | Object persistence, streaming I/O, filesystem layout |
| `save-metadata` | RocksDB operations for buckets, objects, versions |
| `save-common` | Shared types: errors, config, checksums, locks |
| `save-cli` | CLI tool for administration and testing |

### Data Model

**Metadata (RocksDB)**:
```
bkt:{name}               → bucket record
obj:{bucket}/{key}       → object metadata
mpu:{bucket}/{key}:{id}  → multipart upload state
```

Values: Binary (bincode serialization)

**Filesystem Layout**:
```
data/
├─ objects/
│  └─ {hash[0:2]}/{hash[2:4]}/{hash}  # Content-addressed (3-level sharding)
├─ temp/
│  └─ parts/                            # Multipart temp files
└─ rocksdb/                             # Metadata store
```

### Non-Functional Design

| Concern | Strategy |
|---------|----------|
| **Performance** | Streamed I/O, zero-copy when possible |
| **Reliability** | Atomic operations, crash recovery |
| **Security** | TLS, SigV4 auth |
| **Extensibility** | Modular crates for phased growth |
| **Observability** | Structured logging, Prometheus metrics |

### Testing Strategy

- **Unit tests**: Per-crate validation of individual components
- **Integration tests**: End-to-end S3 API compatibility and correctness
- **Concurrency tests**: Multi-threaded access patterns and race conditions
- **Chaos tests**: Crash recovery and failure injection scenarios
- **Performance benchmarks**: Throughput and latency characterization
- **Load tests**: Realistic workload simulation and stress testing

---

## Phase 1: Single-Node Architecture

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

---

## Phase 2: Distributed Architecture

### Data Flow

```
[S3 Client]
    ↓
[API Service - HTTP (axum)] ──────┐
    ↓                              │
[Distributed Lock Manager]         │
    ↓                              │
[Metadata Layer]                   │ Replication
 ├─ Raft Consensus                 │ Coordinator
 └─ RocksDB (replicated)           │
    ↓                              ↓
[Storage Backend Abstraction]  [Internal gRPC API]
 ├─ Replica Placement              ↓
 └─ Quorum Writes              [Node 1, Node 2, Node 3]
    ↓                          (Parallel replica writes)
[Object Data (distributed FS)]
```

### Components
- **Raft Consensus**: Embedded openraft for distributed metadata coordination
- **Distributed Locks**: Replace local locks with Raft-based distributed locking
- **Replication Coordinator**: Manages quorum writes across N replica nodes
- **Storage Backend Abstraction**: Pluggable local/distributed storage
- **Internal gRPC API**: Node-to-node replication protocol (port 9002, mTLS)
- **Replica Placement**: Round-robin node selection (Phase 2), consistent hashing (Phase 3+)
- **Cluster Membership**: Raft-managed node discovery and health tracking

### Key Guarantees
- Linearizable consistency for metadata via Raft consensus
- Quorum-based durability: `floor(N/2) + 1` nodes must ack writes
- Failure tolerance: `floor(N/2)` node failures (e.g., 3-node tolerates 1 failure)
- Read consistency: Eventual (default) or strong (configurable via header)
- Write atomicity: Quorum data write → Raft metadata commit

### Replication Flow
1. Client sends PUT request to any node
2. Node acquires distributed lock for (bucket, key)
3. Query Raft leader for replica node placement
4. Parallel stream to N replicas via internal gRPC
5. Wait for quorum responses (majority ack)
6. Commit metadata via Raft (atomically add to metadata store)
7. Release distributed lock, return success to client

### Data Model Extensions
- Object metadata includes `replica_nodes: Vec<NodeId>` to track which nodes store replicas

### Design Documentation
- [ADR-006](adrs/006-distributed-metadata-strategy.md): Distributed metadata strategy (embedded Raft)
- [REPLICATION_PROTOCOL.md](REPLICATION_PROTOCOL.md): Replication protocol design (push-based, quorum writes)

---

## Future Evolution

Phase 3-4 roadmap available in [ROADMAP.md](ROADMAP.md):
- **Phase 3**: Erasure coding, healing, rebalancing
- **Phase 4**: Multi-tenant IAM, metrics, management API
