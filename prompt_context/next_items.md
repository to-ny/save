# Next items for phase 2

## Priority 1: Metadata Evolution

Foundation for distributed operations. Required before full cluster writes.

From phase2.md:
- [ ] Add `replica_nodes` field to ObjectMetadata struct
- [ ] Extend metadata serialization to include replica information
- [ ] Update all metadata write operations to go through Raft
- [ ] Implement distributed metadata reads (local vs leader)

---

## Priority 2: Distributed Lock Manager

Replace local locking with cluster-wide coordination.

From phase2.md:
- [ ] Implement distributed lock manager using Raft
- [ ] Replace ObjectLockManager with distributed lock implementation
- [ ] Ensure lock API remains unchanged (per ADR-003)

---

## Priority 3: Storage Backend Completion

Enable production-ready replica selection and consistency.

From phase2.md:
- [ ] Add storage backend factory based on cluster config
- [ ] Add replica selection from healthy nodes only
- [ ] Implement replica preference logic (local > remote)
- [ ] Handle remote object reads (proxy to replica nodes via gRPC)
- [ ] Add consistency level support (eventual vs strong reads)
- [ ] Implement streaming replication for large objects

---

## Priority 4: Cluster Coordination Hardening

From phase2.md:
- [ ] Add cluster state tracking (node health, leader status)
- [ ] Implement node discovery on startup from peer configuration
- [ ] Add heartbeat mechanism for node health monitoring
- [ ] Handle network partition detection and recovery
- [ ] Implement split-brain prevention logic
- [ ] Add cluster topology management

---

## Priority 5: Replication Resilience

From phase2.md:
- [ ] Add retry with exponential backoff for transient replication failures
- [ ] Implement mTLS certificate management for node authentication

---

## Priority 6: Observability

From phase2.md:
- [ ] Add Raft-specific metrics (leader elections, log entries, snapshots)
- [ ] Implement replication metrics (writes/reads per node, quorum success/failures)
- [ ] Add cluster health metrics (node status, replication lag)
- [ ] Create replica count metrics (per bucket, under-replicated objects)
- [ ] Add gRPC metrics (request latency, stream duration)
- [ ] Implement distributed tracing across nodes

---

## Priority 7: Testing

From phase2.md:
- [ ] Integration test: Write with node failure (quorum still met)
- [ ] Integration test: Write with quorum failure
- [ ] Integration test: Read-after-write consistency
- [ ] Integration test: Strong consistency reads during network delay
- [ ] Chaos test: Random node failures during write workload
- [ ] Chaos test: Network partition during multipart upload
- [ ] Load test: Multi-node cluster with replication overhead
- [ ] Performance test: Replication latency P50/P90/P99
- [ ] Benchmark: Compare Phase 1 vs Phase 2 write throughput
