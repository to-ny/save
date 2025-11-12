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

### Design rationale
- **Content-addressable storage with SHA256**: Objects stored by hash prevents deduplication issues and enables built-in integrity checking via ETags.
- **RocksDB for metadata**: Provides ACID durability guarantees and atomic batch operations (WriteBatch) needed for transactional metadata updates.

### Error handling
- **S3-compatible XML responses**: All API errors return XML in AWS S3 format with proper error codes (NoSuchBucket, NoSuchKey, AccessDenied, etc.).
- **Request IDs**: Every error response includes a unique request ID (UUID v4) for request correlation across logs, metrics, and client retries.
- **Internal error logging**: Errors are logged at creation time with full context using `tracing::error!`, while client responses are sanitized to prevent information leakage.
- **Structured error types**: Internal errors (`ApiError`) use `thiserror` for rich error context, then convert to S3-compliant responses at the HTTP boundary.
- **HTTP status code mapping**: Error codes map to appropriate HTTP statuses (404 for NoSuchBucket/NoSuchKey, 403 for AccessDenied, 409 for conflicts, 400 for invalid requests, 500 for internal errors).

### Concurrency control
**Objective**: Prevent data corruption when multiple concurrent operations target the same object.

#### Per-Object Locking
- **Implementation**: `ObjectLockManager` in `save-common` provides per-key locks using `tokio::sync::Mutex`.
- **Scope**: Locks are scoped to (bucket, key) tuple — concurrent writes to different objects proceed in parallel without contention.
- **RAII Guards**: Locks automatically released when guard drops, ensuring cleanup even on errors or panics.
- **Timeout**: 30-second default timeout prevents deadlocks.
- **Memory Management**: Unused locks are automatically cleaned up to prevent memory leaks.

#### Operations Protected by Locks
1. **PUT Object** (`save-api/src/handlers/objects/put.rs`):
   - Acquires lock at start of request
   - Serializes concurrent PUTs to same key
   - Lock held until storage + metadata commit completes

2. **Complete Multipart Upload** (`save-api/src/handlers/multipart/complete.rs`):
   - Acquires lock before assembly
   - Prevents concurrent completions of different uploads to same key
   - Lock held until final object committed

3. **DELETE Object** (`save-api/src/handlers/objects/delete.rs`):
   - Acquires lock before existence check
   - Serializes concurrent DELETEs to same key
   - Lock held until metadata + storage deletion completes

#### Commit Ordering Strategy
**Write Operations (PUT/Multipart Complete)**: Storage commits BEFORE metadata to prevent phantom objects.

**Ordering for Writes**:
1. Write object data to temp file (fsynced)
2. **Commit storage** (atomic rename to final path + fsync directory)
3. **Commit metadata** (WriteBatch with sync=true to RocksDB)

**Why This Order**:
- If storage commit fails → metadata never written (consistent state, no phantom object)
- If metadata commit fails → orphaned storage file (acceptable, garbage-collectable)
- **Previous ordering** (metadata first) could create phantom objects: metadata pointing to non-existent storage if storage commit failed

**Delete Operations (DELETE)**: Metadata deleted BEFORE storage (opposite of writes).

**Ordering for Deletes**:
1. **Delete metadata** (atomic RocksDB operation)
2. **Delete storage** (filesystem unlink)

**Why This Order**:
- If metadata deletion fails → object still exists (consistent state, retry possible)
- If storage deletion fails → orphaned storage file (acceptable, garbage-collectable)
- **Wrong ordering** (storage first) could create phantom objects: metadata pointing to deleted storage if storage deletion succeeded but metadata deletion failed

**Edge Cases**:
- Concurrent GET during PUT: Always returns complete old or new version (never partial data)
- Crash during commit: Either fully written or not present (no torn writes)
- Orphaned objects: Cleaned up by GC worker based on temp file age

#### Phase 2 Migration Path
For distributed operation:
- **Local locks** (current) → **Distributed locks** (etcd/Redis)
- API unchanged: `lock_manager.acquire_lock(bucket, key)` abstraction remains
- Swap implementation in `ObjectLockManager` without touching handler code

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

