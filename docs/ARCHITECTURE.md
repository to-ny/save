# Architecture

**Project goal:**  
`save` is a distributed object storage system inspired by MinIO and Amazon S3.  
It begins as a single-node S3-compatible store (Phase 1) and will evolve into a distributed, erasure-coded, self-healing cluster.

---

## 1. System Overview

### Core principles
- **S3-compatible API** — predictable external behavior.
- **Layered modularity** — separate crates for API, storage, metadata, and shared utilities.
- **Progressive expansion** — single-node correctness first; distributed reliability later.
- **Rust safety + async I/O** — leverage `tokio`, `tracing`, and `serde` for reliable performance.

### High-level data flow

[S3 Client]
│
▼
[API Service ─ HTTP (axum)]
│
▼
[Storage Layer]
├─ Metadata (RocksDB)
└─ Object Data (local FS)

Each request passes through authentication, validation, and routing to the storage subsystem, which manages metadata in RocksDB and object bytes on the local filesystem.

---

## 2. Crate layout (planned)

| Crate | Purpose |
|-------|----------|
| `save-api` | Implements S3-compatible REST API (PUT/GET/DELETE, multipart, auth). |
| `save-storage` | Handles object persistence, streaming I/O, and file layout. |
| `save-metadata` | Encapsulates RocksDB operations for buckets, objects, versions. |
| `save-common` | Shared types: errors, config, checksums, tracing helpers. |

Each crate is self-contained and tested independently.

---

## 3. Phase 1 — Single-node architecture

### Components
- **HTTP Frontend**: `axum`-based S3 handler.
- **Auth Layer**: SigV4 validation with static credentials.
- **Metadata Layer**: RocksDB for bucket/object metadata.
- **Storage Engine**: Local FS object layout (content-addressed).
- **Multipart Coordinator**: State machine for multi-part uploads.
- **GC Worker**: Periodic cleanup of orphaned parts/files.
- **Observability**: `/metrics`, `/health`, structured logging.

### Key guarantees
- **Strong consistency** (single node).
- **Atomic PUT** via temp file + atomic rename.
- **Crash recovery** using RocksDB durability and file rename semantics.

---

## 4. Phase 2–4 preview (planned evolution)

| Phase | Focus | New Components |
|-------|--------|----------------|
| **2** | Replication & Cluster Coordination | Raft/etcd cluster, membership, state sync |
| **3** | Erasure Coding & Healing | Shard placement, reconstruction, rebalancing |
| **4** | Multi-Tenant & Operator Layer | IAM policies, metrics, CLI, management API |

Each phase builds on stable APIs and metadata formats from earlier stages.

---

## 5. Data model (simplified)

### Metadata (RocksDB)
- `bkt:{name}` → bucket record
- `obj:{bucket}/{key}` → object metadata
- `mpu:{bucket}/{object}:{upload_id}` → multipart upload state
- Values stored as compact binary (protobuf or bincode).

### Filesystem layout

data/
├─ objects/ab/12/...      # object bytes, sharded by prefix
├─ temp/parts/...         # multipart temp files
└─ rocksdb/               # metadata store

---

## 6. Non-functional design

| Concern | Strategy |
|----------|-----------|
| **Performance** | Streamed I/O, no buffering large objects in memory. |
| **Reliability** | Atomic metadata + file move semantics. |
| **Security** | TLS, SigV4 auth, static credentials initially. |
| **Extensibility** | Crate-based modular boundaries for phased growth. |
| **Observability** | `tracing`, Prometheus metrics, structured logs. |

---

## 7. Development conventions

- **Async everywhere**: all I/O via `tokio`.
- **Error handling**: `anyhow` at API level, `thiserror` in libraries.
- **Configuration**: TOML-based, parsed with `serde`.
- **Testing**:
  - Unit tests inside crates.
  - Integration tests under `/tests` using local FS.
- **Documentation**: Each crate has its own `README.md` describing public interfaces.

---

## 8. Future Considerations
- Cluster metadata consistency (Raft/etcd integration).
- Object placement and healing scheduler.
- Encryption at rest (SSE-KMS integration).
- S3 feature completeness (ACLs, versioning, lifecycle policies).
- CLI tools for ops and debugging.

---

_Last updated: {{today}}_

