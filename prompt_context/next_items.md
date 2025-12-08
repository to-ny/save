# Next items for phase 2

## Priority 1: Observability

From phase2.md:
- [ ] Add Raft-specific metrics (leader elections, log entries, snapshots)
- [ ] Implement replication metrics (writes/reads per node, quorum success/failures)
- [ ] Add cluster health metrics (node status, replication lag)
- [ ] Create replica count metrics (per bucket, under-replicated objects)
- [ ] Add gRPC metrics (request latency, stream duration)
- [ ] Implement distributed tracing across nodes

---

## Priority 2: Testing

Integration & load tests (require multi-node setup):
- [ ] Integration test: Write with node failure (quorum still met)
- [ ] Integration test: Write with quorum failure
- [ ] Integration test: Read-after-write consistency
- [ ] Integration test: Strong consistency reads during network delay
- [ ] Chaos test: Random node failures during write workload
- [ ] Chaos test: Network partition during multipart upload
- [ ] Load test: Multi-node cluster with replication overhead
- [ ] Performance test: Replication latency P50/P90/P99
