# ADR-003: Per-Object Locking Strategy

**Status**: Accepted

## Context

Concurrent operations on the same object can cause data corruption, race conditions, or inconsistent state between metadata and storage.

## Decision

Implement per-object locking using `ObjectLockManager`:
- Scope: Lock keyed by (bucket, key) tuple
- Implementation: `tokio::sync::RwLock` with 30-second timeout
- RAII guards for automatic cleanup
- Automatic memory management (unused locks removed)

**Protected operations**:
- Write locks: PUT, Complete Multipart Upload, DELETE
- Read locks: Available for future read operations requiring consistency

## Consequences

**Positive**:
- Prevents data corruption from concurrent writes
- Allows parallel operations on different objects
- Allows concurrent reads on same object (RwLock benefit)
- Automatic cleanup on errors/panics via RAII
- Deadlock prevention via timeout
- Memory leak prevention via automatic cleanup

**Negative**:
- Single-node only (Phase 1 limitation)
- Write lock contention on hot objects
- 30-second timeout may be too long for some workloads
- Memory overhead for lock tracking

**Phase 2 Migration**:
- Replace `tokio::RwLock` with distributed lock (Raft-based)
- API abstraction (`lock_manager.acquire_write_lock`) remains unchanged
- Handler code unaffected
