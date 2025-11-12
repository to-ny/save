# ADR-004: Storage-Before-Metadata Commit Ordering

**Status**: Accepted

## Context

Write and delete operations involve both storage and metadata updates. The order of these commits determines which failure modes are possible and which invariants can be maintained.

## Decision

**Writes (PUT/Complete Multipart)**: Storage commits BEFORE metadata
1. Write temp file + fsync
2. Atomic rename to final path + fsync directory (storage commit)
3. WriteBatch to RocksDB with sync=true (metadata commit)

**Deletes (DELETE)**: Metadata deleted BEFORE storage
1. Delete from RocksDB (metadata commit)
2. Unlink filesystem file (storage commit)

## Consequences

**Writes (Storage-Before-Metadata)**:
- Storage commit fails → metadata never written (consistent, no phantom object)
- Metadata commit fails → orphaned storage file (acceptable, GC'd later)
- Prevents phantom objects (metadata pointing to missing storage)

**Deletes (Metadata-Before-Storage)**:
- Metadata deletion fails → object still exists (consistent, retry possible)
- Storage deletion fails → orphaned storage file (acceptable, GC'd later)
- Prevents phantom objects

**Key Invariant**: No phantom objects (metadata without storage) ever exist.

**Acceptable State**: Orphaned storage (storage without metadata) is acceptable and cleaned by GC.
