# Next items for phase 2

## Priority 1: Code Quality & Architecture

Recommendations from codebase analysis:

### Module Refactoring (Large Files)
- [x] Split `raft/storage.rs` (788→427 LOC) into: log_store.rs, state_machine.rs
- [x] Extract health monitoring from `replication/coordinator.rs` (715→695 LOC) into: health.rs
- [ ] Consider splitting `replicated_backend.rs` (701 LOC) if it grows further

### Benchmarks
- [ ] Add replication benchmarks (2PC overhead, streaming performance)
- [ ] Add cluster coordination benchmarks (heartbeat, partition detection)
- [ ] Add health check benchmarks

### Ongoing Maintenance
- [ ] Monitor module sizes (maintain <500 LOC guideline)
- [ ] Increase unit test coverage in save-storage and save-metadata

---

## Priority 2: Replication Resilience

From phase2.md:
- [ ] Add retry with exponential backoff for transient replication failures
- [ ] Implement mTLS certificate management for node authentication

---

## Priority 3: Observability

From phase2.md:
- [ ] Add Raft-specific metrics (leader elections, log entries, snapshots)
- [ ] Implement replication metrics (writes/reads per node, quorum success/failures)
- [ ] Add cluster health metrics (node status, replication lag)
- [ ] Create replica count metrics (per bucket, under-replicated objects)
- [ ] Add gRPC metrics (request latency, stream duration)
- [ ] Implement distributed tracing across nodes

---

## Priority 4: Testing

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
- [ ] Benchmark: Cluster coordination overhead

---

## Completed

### Cluster Coordination Hardening (Done)
- [x] Add cluster state tracking (node health, leader status)
- [x] Implement node discovery on startup from peer configuration
- [x] Add heartbeat mechanism for node health monitoring
- [x] Handle network partition detection and recovery
- [x] Implement split-brain prevention logic
- [x] Add cluster topology management

### Storage Backend Completion (Done)
- [x] Add storage backend factory based on cluster config
- [x] Add replica selection from healthy nodes only
- [x] Implement replica preference logic (local > remote)
- [x] Handle remote object reads (proxy to replica nodes via gRPC)
- [x] Implement streaming replication for large objects
- [x] Add configurable health check timeout
- [x] Use read_quorum in read_from_replica

### Code Quality Improvements (Done)
- [x] Extract shared 2PC logic from replicate_write methods (process_prepare_results, complete_2pc_write)
- [x] Improve stream_prepare_object temp handling (added create_temp_object method)
- [x] Fix gRPC read_object to return NOT_FOUND error instead of empty response
- [x] Add health_status constants module

### Distributed Lock Manager (Done)
- [x] Implement distributed lock manager using Raft
- [x] Replace ObjectLockManager with distributed lock implementation
- [x] Ensure lock API remains unchanged (per ADR-003)

### Metadata Evolution (Done)
- [x] Add `replica_nodes` field to ObjectMetadata struct
- [x] Extend metadata serialization to include replica information
- [x] Update all metadata write operations to go through Raft
- [x] Implement distributed metadata reads (local vs leader)
- [x] Add consistency level support (eventual vs strong reads)
