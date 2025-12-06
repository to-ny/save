# Next Items - Phase 2

## Current Status

**Raft Consensus Layer** - Core implementation complete:
- RocksDB storage with column families (raft_log, raft_state, raft_snapshot)
- Log persistence, state persistence, command application
- gRPC client/server for Raft RPCs (proto/raft.proto)
- Network layer (RaftNetworkFactory trait)
- ClusterConfig integration
- Snapshot generation (RocksDB checkpoint) and streaming RPC

**Replication Infrastructure** - Complete:
- proto/replication.proto with WriteReplica/ReadReplica/DeleteReplica/ReplicationHealth services
- ReplicationService, ReplicationClient, ReplicationCoordinator implemented
- 2PC (PrepareObject/CommitObject/AbortObject) for quorum writes

**Storage Backend** - Complete:
- StorageBackend trait with dyn-compatible interface
- LocalBackend wrapper for single-node
- ReplicatedBackend with 2PC quorum writes
- API handlers use `Arc<dyn StorageBackend>`

---

## Next Priorities

### Priority 1: Raft Snapshots ✓

From phase2.md:
- [x] Implement Raft snapshot generation for state machine
- [x] Add snapshot transfer mechanism for new/recovering nodes

Completed:
- `snapshot.rs`: RocksDB checkpoint, tar.gz packaging, install/restore
- Streaming snapshot RPC wired in server.rs
- 5 unit tests for snapshot operations

### Priority 2: Leader Election Monitoring + Cluster Tests ✓

From phase2.md:
- [x] Add leader election monitoring and status tracking

Completed:
- `ClusterStatus` struct in save-metadata/src/raft/node.rs
- `/cluster/status` endpoint in save-api (GET)
- `/cluster/initialize` endpoint in save-api (POST)
- `ClusterEnv` test infrastructure in tests/crash-recovery/tests/common/cluster.rs
- Main.rs now initializes Raft when cluster mode is enabled
- MetadataStore exposes `db()` method for Raft sharing

Tests (from phase2.md):
- [x] Integration test: 3-node cluster formation
- [x] Integration test: Leader election after leader crash
- [x] Integration test: Node recovery and catch-up
- [x] Integration test: Cluster survives minority failure (bonus)

Run tests with: `cargo test -p crash-recovery-tests --features cluster_tests cluster --ignored`

### Priority 3: Replication Service ✓

From phase2.md:
- [x] Implement ReplicationService gRPC server
- [x] Add WriteReplica/ReadReplica/DeleteReplica RPC handlers
- [x] Create ReplicationCoordinator for quorum writes

Completed:
- `save-storage/src/replication/service.rs`: ReplicationService handling 2PC (PrepareObject/CommitObject/AbortObject), direct writes, reads, deletes
- `save-storage/src/replication/client.rs`: ReplicationClient with tonic gRPC codec
- `save-storage/src/replication/coordinator.rs`: ReplicationCoordinator with QuorumConfig, parallel writes, 2PC orchestration
- `save-storage/src/replication/server.rs`: gRPC server with graceful shutdown

Tests (from phase2.md):
- [x] Unit tests for ReplicationCoordinator quorum logic
- [x] Unit tests for replica placement strategy (basic round-robin)

### Priority 4: ReplicatedBackend ✓

From phase2.md:
- [x] Create ReplicatedBackend implementation
- [x] Update API handlers to use StorageBackend trait

Completed:
- `save-storage/src/replicated_backend.rs`: ReplicatedBackend with 2PC via ReplicationCoordinator
- `save-storage/src/error.rs`: Added `QuorumNotAchieved` error variant
- `save-storage/src/backend.rs`: TempHandle contract documented
- `save-api/src/state.rs`: AppState uses `Arc<dyn StorageBackend>`
- `save-api/src/main.rs`: Uses LocalBackend (ReplicatedBackend via config TBD)

Tests:
- [x] Integration test: Write with all replicas healthy
- [x] Integration test: Write with node failure (quorum still met)
- [x] Integration test: Write with quorum failure
- [x] Integration test: Read-after-write consistency
- [x] Integration test: Delete replicates to nodes
- [x] Integration test: Concurrent writes to replicas

### Priority 5: Cluster Membership ✓

From phase2.md:
- [x] Implement cluster membership management (add/remove nodes)
- [x] Handle Raft configuration changes

Completed:
- `save-metadata/src/raft/node.rs`: add_learner(), promote_voters(), remove_voters(), remove_node()
- `save-api/src/routes/cluster.rs`: POST /cluster/members, POST /cluster/members/promote, DELETE /cluster/members/{node_id}
- ClusterStatus now includes `voters` and `learners` lists
- Unit tests for new endpoints
- Integration test: test_remove_node_from_cluster

Tests (from phase2.md):
- [x] Integration test: Snapshot transfer to new node
- [x] Integration test: Network partition (split-brain prevention)
- [x] Integration test: Concurrent writes to same object (distributed locking)

