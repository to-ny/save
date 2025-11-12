# ADR-001: Content-Addressable Storage

**Status**: Accepted

## Context

Object storage requires a reliable filesystem layout that supports deduplication detection, integrity verification, and efficient lookups.

## Decision

Store objects by SHA256 hash of (bucket, key) tuple in a sharded directory structure.

**Layout**: `data/objects/{hash[0:2]}/{hash[2:4]}/{hash}/`

## Consequences

**Positive**:
- Built-in integrity checking via ETags
- Prevents accidental overwrites from path collisions
- Enables future deduplication detection
- Deterministic object paths for verification

**Negative**:
- Hash computation overhead on every write
- Cannot derive original key from filesystem (requires metadata lookup)
- Directory traversal needed for orphan detection
