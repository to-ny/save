# Replication Protocol Design

Phase 2 replication architecture for multi-node object storage.

---

## Overview

- **Model**: Push-based synchronous replication with quorum writes
- **Consistency**: Strong consistency via Raft-coordinated metadata + quorum data writes
- **Replication Factor**: Configurable (default: 3)
- **Failure Tolerance**: `floor(N/2)` nodes

---

## Architecture

### Cluster Roles

**Metadata Leader** (Raft-elected): Coordinates metadata writes, assigns replica placement, tracks node health

**Data Nodes** (all): Store replicas locally, serve reads, participate in replication

### Write Path

1. Acquire distributed lock (bucket/key)
2. Query Raft leader for replica node placement
3. Parallel stream to N replicas via gRPC
4. Wait for quorum acks (majority)
5. Commit metadata via Raft
6. Release lock, return success

**Decisions**: Push model, synchronous quorum, round-robin placement (Phase 2)

### Read Path

1. Query metadata from local RocksDB (eventual consistency)
2. Check local replica availability
3. Stream from local storage OR proxy to remote replica
4. Return object

**Optimization**: Prefer local replicas (zero network hops)

---

## Internal Replication API

**gRPC service** (port 8081, mTLS):
- `WriteReplica`: Streaming object write to replica
- `ReadReplica`: Streaming object read from replica
- `DeleteReplica`: Remove replica
- `ReplicationHealth`: Node health check

**Write Protocol**:
1. **Prepare**: Acquire lock, query placement
2. **Replicate**: Parallel gRPC streams to N nodes, wait for quorum acks
3. **Commit**: Raft metadata commit (includes replica_node_ids)
4. **Finalize**: Release lock, replicas rename temp → final

**Failure Handling**:
- Quorum fails: Abort, cleanup partial writes, return error
- Node fails mid-stream: Timeout (5s), exclude from quorum
- Metadata commit fails: Retry (data already durable)

---

## Quorum Strategies

**Write Quorum** (default: majority)

| Replication Factor | Quorum | Failure Tolerance |
|--------------------|--------|-------------------|
| 3                  | 2      | 1                 |
| 5                  | 3      | 2                 |
| 7                  | 4      | 3                 |

**Read Quorum**:
- Default: R=1 (eventual consistency)
- Strong: R=majority (via `X-Consistency-Level` header)

---

## Failure Scenarios

**Node Failure During Write**: Quorum succeeds with fewer replicas (2/3), background healing creates missing replica

**Leader Failure**: Client timeout, Raft elects new leader (<1s), client retries

**Network Partition**: Majority partition continues, minority rejects writes (no quorum)

**Conflicting Writes**: Distributed lock serializes, last write wins

---

## Performance

**Write Amplification**: N× network bandwidth (mitigated by erasure coding in Phase 3)

**Latency Overhead** (3-node, 1ms RTT):
- PUT: +7ms (lock +2ms, replication +5ms, Raft +3ms)
- GET (local): 0ms
- GET (remote): +2ms
- DELETE: +5ms

---

## Monitoring

**Key Metrics**:
- `save_replication_writes_total{status}`: Write outcomes
- `save_replication_lag_seconds{node_id}`: Replication lag
- `save_cluster_nodes_total{status}`: Node health
- `save_replica_count{under_replicated}`: Replication status
- `save_quorum_failures_total`: Quorum failures

**Alerts**:
- Critical: Quorum failures > 10/min
- Warning: Under-replicated objects > 100
- Info: Healthy nodes < replication_factor

---

## Migration from Phase 1

**Single-Node to Cluster**:
1. Stop node, copy RocksDB + objects to new nodes
2. Enable `cluster.enabled = true`
3. Start 3-node cluster, Raft bootstraps
4. Background process marks existing objects as replicated

**Gradual (Zero Downtime)**:
1. Enable cluster mode with replication_factor=1 (no replication yet)
2. Add nodes incrementally, increase replication_factor
3. Background healing replicates old objects

---

## Placement Strategy

**Phase 2**: Round-robin over healthy nodes
**Phase 3**: Consistent hashing for better rebalancing

---

## Future Enhancements

- Erasure coding (reduce 3× to ~1.5× storage)
- Read repair (background stale replica detection)
- Async replication mode (lower durability, higher throughput)
- Geo-replication (cross-datacenter)
