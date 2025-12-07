# ADR-003: Per-Object Locking Strategy

**Status**: Accepted

## Context

Concurrent operations on the same object can cause data corruption, race conditions, or inconsistent state between metadata and storage.

## Decision

Implement per-object locking with RAII guards:
- Scope: Lock keyed by (bucket, key) tuple
- RAII guards for automatic cleanup (`DistributedReadLockGuard`, `DistributedWriteLockGuard`)
- 30-second timeout with retry-on-contention
- Read/write lock semantics (multiple readers OR single writer)

**Protected operations**:
- Write locks: PUT, Complete Multipart Upload, DELETE
- Read locks: GET, HEAD

**API**:
```rust
lock_manager.acquire_read_lock(bucket, key).await   // Returns guard
lock_manager.acquire_write_lock(bucket, key).await  // Returns guard
// Lock released automatically when guard drops
```

## Implementation

### Phase 1 (Single-Node)
- `ObjectLockManager` using `tokio::sync::RwLock`
- In-memory lock tracking with automatic cleanup

### Phase 2 (Distributed)
- `DistributedLockManager` using Raft consensus
- Lock state stored in RocksDB via Raft state machine
- `AcquireLock` and `ReleaseLock` Raft commands
- Retry with backoff on contention until timeout
- Lock guards spawn async release on drop

## Consequences

**Positive**:
- Prevents data corruption from concurrent writes
- Allows parallel operations on different objects
- Allows concurrent reads on same object
- Automatic cleanup on errors/panics via RAII
- Deadlock prevention via timeout
- Cluster-wide coordination (Phase 2)
- Handler code unchanged between phases

**Negative**:
- Write lock contention on hot objects
- 30-second timeout may be too long for some workloads
- Raft round-trip overhead for lock acquisition (Phase 2)
