# ADR-002: RocksDB for Metadata Store

**Status**: Accepted

## Context

Metadata storage requires ACID guarantees, atomic batch operations, and crash-safe durability for bucket and object metadata.

## Decision

Use RocksDB as the embedded metadata store with:
- Synchronous writes (`WriteOptions::sync = true`)
- WriteBatch for atomic multi-key updates
- Key prefix scheme: `bkt:{name}`, `obj:{bucket}/{key}`, `mpu:{bucket}/{key}:{upload_id}`

## Consequences

**Positive**:
- ACID durability guarantees
- Atomic batch operations for transactional updates
- No external database dependency
- Proven stability (used by TiKV, CockroachDB)
- Efficient prefix scans for listing operations

**Negative**:
- Single-node only (Phase 1 limitation)
- No distributed transactions (Phase 2 will require etcd/Raft)
- Memory overhead for LSM-tree compaction
- Migration complexity when moving to distributed metadata
