# ADR-006: Distributed Metadata Strategy

**Status**: Proposed

## Context

Phase 2 requires distributed metadata to support multi-node replication with strong consistency. Requirements: cluster membership, distributed coordination, atomic metadata operations, metadata replication, migration from single-node RocksDB.

## Decision

Use **embedded Raft consensus** (`openraft` crate) with RocksDB as state machine storage.

**Architecture**:
- Each node runs embedded Raft instance
- RocksDB replicates via Raft log
- Metadata writes through Raft leader (linearizable)
- Metadata reads from local RocksDB (eventual) or leader (strong)
- gRPC for node-to-node Raft communication

## Alternatives Considered

**etcd (External Service)**:
- Pros: Battle-tested, simpler integration, built-in tooling
- Cons: External dependency, network latency, not self-contained
- Rejected: Architectural mismatch with MinIO/Ceph design philosophy

**TiKV Model (Distributed RocksDB)**:
- Pros: Highest scalability, region-based sharding
- Cons: Extreme complexity, requires Placement Driver, 6+ month effort
- Rejected: Overkill for Phase 2

## Consequences

**Positive**:
- Self-contained (no external services)
- Low latency (local reads, single hop writes)
- Clean migration (RocksDB format unchanged)
- Strong consistency guarantees
- Production-ready Rust ecosystem (`openraft`)

**Negative**:
- Raft implementation complexity (consensus, snapshots)
- Leader bottleneck for writes
- Snapshot transfer overhead
- Memory overhead for Raft log

## Migration Plan

**Single-Node → Cluster**:
1. Copy RocksDB to new nodes
2. Bootstrap Raft from config

**Deployment Models**:
- Small (1-3 nodes): Single Raft group, all metadata replicated
- Large (Phase 3+): Sharded metadata, multiple Raft groups

## Configuration

**Consistency**:
- Strong: Read from leader (higher latency)
- Eventual: Read from local (stale possible)

**Quorum**: `floor(N/2) + 1` majority, recommend odd cluster sizes (3, 5, 7)
