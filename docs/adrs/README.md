# Architecture Decision Records

This directory contains Architecture Decision Records (ADRs) documenting key design decisions for the save object storage system.

## Active ADRs

| ADR | Title | Status |
|-----|-------|--------|
| [001](001-content-addressable-storage.md) | Content-Addressable Storage | Accepted |
| [002](002-rocksdb-metadata-store.md) | RocksDB for Metadata Store | Accepted |
| [003](003-per-object-locking.md) | Per-Object Locking Strategy | Accepted |
| [004](004-commit-ordering.md) | Storage-Before-Metadata Commit Ordering | Accepted |
| [005](005-s3-compatible-error-handling.md) | S3-Compatible Error Handling | Accepted |

## ADR Format

Each ADR follows this structure:
- **Status**: Accepted, Superseded, Deprecated
- **Context**: Why the decision was needed
- **Decision**: What was decided
- **Consequences**: Trade-offs and implications
