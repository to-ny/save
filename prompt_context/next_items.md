# Next items for phase 2

## Priority 1: Internal APIs

From phase2.md:
- [ ] Add internal gRPC server startup (port 8082)
- [ ] Implement gRPC middleware for request logging
- [ ] Add gRPC interceptors for authentication (mTLS validation)
- [ ] Create internal API for cluster management operations
- [ ] Add Raft-specific endpoints (add node, remove node, leader transfer)
- [ ] Implement node drain API for graceful shutdown
- [ ] Add debug endpoints for cluster state inspection

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
