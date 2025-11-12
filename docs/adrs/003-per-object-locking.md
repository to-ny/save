# ADR-003: Per-Object Locking Strategy

**Status**: Accepted

## Context

Concurrent operations on the same object can cause data corruption, race conditions, or inconsistent state between metadata and storage.

## Decision

Implement per-object locking using `ObjectLockManager`:
- Scope: Lock keyed by (bucket, key) tuple
- Implementation: `tokio::sync::Mutex` with 30-second timeout
- RAII guards for automatic cleanup
- Automatic memory management (unused locks removed)

**Protected operations**: PUT, Complete Multipart Upload, DELETE

## Consequences

**Positive**:
- Prevents data corruption from concurrent writes
- Allows parallel operations on different objects
- Automatic cleanup on errors/panics via RAII
- Deadlock prevention via timeout
- Memory leak prevention via automatic cleanup

**Negative**:
- Single-node only (Phase 1 limitation)
- Lock contention on hot objects
- 30-second timeout may be too long for some workloads
- Memory overhead for lock tracking

**Phase 2 Migration**:
- Replace `tokio::Mutex` with distributed lock (etcd/Redis)
- API abstraction (`lock_manager.acquire_lock`) remains unchanged
- Handler code unaffected
