# ADR-001: Content-Addressable Storage

**Status**: Accepted

## Context

Object storage requires a reliable filesystem layout that supports integrity verification and efficient lookups. Bucket/key mapping is handled at the metadata layer.

## Decision

Store objects by SHA256 hash of the object key in a sharded directory structure.

**Layout**: `data/objects/{hash[0:2]}/{hash}`

**Note**: Bucket information is stored in metadata layer only, not in the hash. The storage key passed to the storage layer is `{bucket}/{key}`, and this full string is hashed.

## Consequences

**Positive**:
- Built-in integrity checking via ETags
- Deterministic object paths for verification
- Simple 2-level directory sharding reduces filesystem overhead
- Bucket isolation handled at metadata/API layer

**Negative**:
- Hash computation overhead on every write
- Cannot derive original key from filesystem (requires metadata lookup)
- Directory traversal needed for orphan detection
