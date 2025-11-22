# Phase 2 Tasks — Replication & Cluster Coordination

## Goal
Transform the single-node S3-compatible store into a distributed, replicated system with cluster coordination. Build on Phase 1's foundation to enable multi-node deployments with strong consistency guarantees.

---

## Preparation & Design
- [x] Create ADR-006: Distributed metadata strategy (Location: `docs/adrs/006-distributed-metadata-strategy.md`)
- [x] Design replication protocol document (Location: `docs/REPLICATION_PROTOCOL.md`)
- [x] Update ARCHITECTURE.md with Phase 2 components and data flows
- [x] Update DESIGN_NOTES.md with Phase 2 implementation notes

---

## Raft Consensus Layer
- [x] Add cluster configuration to SaveConfig (Location: `save-common/src/config.rs`)
- [x] Add openraft dependency to workspace `Cargo.toml`
- [ ] Create Raft node module in `save-metadata` crate
- [ ] Implement RaftStateMachine trait wrapping RocksDB
- [ ] Add Raft log storage using RocksDB column family
- [ ] Implement Raft network layer (node-to-node communication)
- [ ] Add Raft configuration from cluster config (heartbeat, election timeout)
- [ ] Implement cluster membership management (add/remove nodes)
- [ ] Add leader election monitoring and status tracking
- [ ] Implement Raft snapshot generation for state machine
- [ ] Add snapshot transfer mechanism for new/recovering nodes
- [ ] Handle Raft configuration changes (dynamic cluster membership)

---

## Replication Infrastructure
- [x] Create gRPC service definition `proto/replication.proto`
- [x] Generate Rust code from protobuf (tonic-build in build.rs)
- [ ] Implement ReplicationService gRPC server
- [ ] Add WriteReplica RPC handler (streaming writes)
- [ ] Add ReadReplica RPC handler (streaming reads)
- [ ] Add DeleteReplica RPC handler
- [ ] Add ReplicationHealth RPC handler
- [ ] Implement mTLS certificate management for node authentication
- [ ] Create ReplicationCoordinator struct for managing replica writes
- [ ] Implement parallel streaming to N replica nodes
- [ ] Add quorum wait logic with configurable timeout
- [ ] Implement replica placement strategy (round-robin for Phase 2)
- [ ] Add replica selection from healthy nodes only
- [ ] Handle partial write failures and rollback

---

## Storage Backend
- [x] Create StorageBackend trait abstraction (Location: `save-storage/src/backend.rs`)
- [x] Implement LocalBackend wrapper (Location: `save-storage/src/local_backend.rs`)
- [ ] Create ReplicatedBackend implementation (Location: `save-storage/src/replicated_backend.rs`)
- [ ] Integrate ReplicationCoordinator with ReplicatedBackend
- [ ] Add storage backend factory based on cluster config
- [ ] Update API handlers to use StorageBackend trait instead of ObjectStorage directly
- [ ] Handle remote object reads (proxy to replica nodes via gRPC)
- [ ] Implement replica preference logic (local > remote)
- [ ] Add consistency level support (eventual vs strong reads)

---

## Metadata Evolution
- [ ] Add `replica_nodes` field to ObjectMetadata struct
- [ ] Extend metadata serialization to include replica information
- [ ] Create migration utility for Phase 1 → Phase 2 metadata format
- [ ] Update all metadata write operations to go through Raft
- [ ] Implement distributed metadata reads (local vs leader)
- [ ] Add metadata version tracking for compatibility

---

## Cluster Coordination
- [ ] Implement distributed lock manager using Raft
- [ ] Replace ObjectLockManager with distributed lock implementation
- [ ] Ensure lock API remains unchanged (per ADR-003)
- [ ] Add cluster state tracking (node health, leader status)
- [ ] Implement node discovery on startup from peer configuration
- [ ] Add heartbeat mechanism for node health monitoring
- [ ] Handle network partition detection and recovery
- [ ] Implement split-brain prevention logic
- [ ] Add cluster topology management
- [ ] Create cluster status API endpoint `/cluster/status`

---

## Internal APIs
- [ ] Add internal gRPC server startup (port 8082)
- [ ] Implement gRPC middleware for request logging
- [ ] Add gRPC interceptors for authentication (mTLS validation)
- [ ] Create internal API for cluster management operations
- [ ] Add Raft-specific endpoints (add node, remove node, leader transfer)
- [ ] Implement node drain API for graceful shutdown
- [ ] Add debug endpoints for cluster state inspection

---

## Observability & Monitoring
- [ ] Add Raft-specific metrics (leader elections, log entries, snapshots)
- [ ] Implement replication metrics (writes/reads per node, quorum success/failures)
- [ ] Add cluster health metrics (node status, replication lag)
- [ ] Create replica count metrics (per bucket, under-replicated objects)
- [ ] Add gRPC metrics (request latency, stream duration)
- [ ] Implement distributed tracing across nodes (trace IDs propagation)
- [ ] Update Grafana dashboard with cluster panels
- [ ] Add alerting rules for cluster degradation
- [ ] Create replication lag alerts
- [ ] Add quorum failure alerts

---

## Testing
- [ ] Unit tests for Raft state machine integration
- [ ] Unit tests for ReplicationCoordinator quorum logic
- [ ] Unit tests for replica placement strategy
- [ ] Integration test: 3-node cluster formation
- [ ] Integration test: Leader election after leader crash
- [ ] Integration test: Write with node failure (quorum still met)
- [ ] Integration test: Write with quorum failure
- [ ] Integration test: Network partition (split-brain prevention)
- [ ] Integration test: Node recovery and catch-up
- [ ] Integration test: Snapshot transfer to new node
- [ ] Integration test: Concurrent writes to same object (distributed locking)
- [ ] Integration test: Read-after-write consistency
- [ ] Integration test: Strong consistency reads during network delay
- [ ] Chaos test: Random node failures during write workload
- [ ] Chaos test: Network partition during multipart upload
- [ ] Load test: Multi-node cluster with replication overhead
- [ ] Performance test: Replication latency P50/P90/P99
- [ ] Benchmark: Compare Phase 1 vs Phase 2 write throughput

---

## Deployment & Operations
- [ ] Update `save.toml.example` with cluster configuration examples
- [ ] Create cluster deployment documentation (3-node, 5-node setups)
- [ ] Add TLS certificate generation guide for mTLS
- [ ] Document cluster bootstrap procedure
- [ ] Create node addition procedure (scaling up)
- [ ] Create node removal procedure (scaling down)
- [ ] Document cluster upgrade strategy (rolling upgrades)
- [ ] Add troubleshooting guide (partition recovery, replication lag)
- [ ] Create runbook for leader failure scenarios
- [ ] Update Docker Compose for multi-node local testing
- [ ] Create Kubernetes manifests for cluster deployment
- [ ] Add backup/restore procedures for distributed cluster
- [ ] Document disaster recovery scenarios (quorum loss)

---

## Phase 1 Cleanup (Optional but Recommended)
- [ ] Improve directory sharding to 3-level (objects/{hash[0:2]}/{hash[2:4]}/{hash}) for better scalability
- [ ] Wire up rate limiting middleware (config exists, implementation needed)
- [ ] Implement connection limits enforcement
- [ ] Add backpressure handling (503 on overload)
- [ ] Create Makefile/Justfile for common development tasks
- [ ] Add setup/teardown scripts in `scripts/`
- [ ] Complete deployment documentation (systemd, TLS termination)
- [ ] Create operator runbooks for Phase 1 operations

---

## S3 Feature Completeness (Deferred - Can be done in parallel with Phase 2)
- [ ] Implement ACLs (Access Control Lists)
- [ ] Add bucket policies support
- [ ] Implement lifecycle policies (auto-deletion, transitions)
- [ ] Add CORS configuration
- [ ] Implement object tagging and custom metadata
- [ ] Add object versioning (deferred from Phase 1)

---

## Background Healing & Maintenance (Phase 2.1+)
- [ ] Implement healing process to detect under-replicated objects
- [ ] Add healing worker to create missing replicas
- [ ] Implement healing metrics and progress tracking
- [ ] Add healing throttling to avoid overload
- [ ] Create API to trigger manual healing
- [ ] Add healing status endpoint

---

## Notes

### Prerequisites from Phase 1
All Phase 1 critical tasks must be complete before starting Phase 2 core work:
- [x] Full SigV4 authentication
- [x] Atomic metadata+storage operations
- [x] Crash recovery guarantees
- [x] Basic observability (metrics, logging)

### Deferred to Phase 3+
- Erasure coding (replace 3× replication with ~1.5× EC)
- Self-healing and automatic rebalancing
- Data reconstruction after node loss
- Consistent hashing for replica placement
- Cross-datacenter replication

### Deferred to Phase 4+
- Multi-tenant IAM and isolation
- Advanced management API
- Operator tooling and automation
- Web UI for cluster management

### Key Design Decisions
- [ADR-006](../docs/adrs/006-distributed-metadata-strategy.md): Embedded Raft consensus
- [REPLICATION_PROTOCOL.md](../docs/REPLICATION_PROTOCOL.md): Push-based quorum replication
- [ARCHITECTURE.md](../docs/ARCHITECTURE.md): System architecture with Phase 2 components
