# Next Items - Phase 2

## Current Status

**Raft Consensus Layer** - Core implementation complete:
- RocksDB storage with column families (raft_log, raft_state, raft_snapshot)
- Log persistence, state persistence, command application
- gRPC client/server for Raft RPCs (proto/raft.proto)
- Network layer (RaftNetworkFactory trait)
- ClusterConfig integration
- Snapshot generation (RocksDB checkpoint) and streaming RPC

**Replication Infrastructure** - Proto defined, implementation pending:
- proto/replication.proto exists
- No service implementation yet

**Storage Backend** - Trait abstraction complete:
- StorageBackend trait and LocalBackend wrapper done
- ReplicatedBackend not started

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

### Priority 3: Replication Service

From phase2.md:
- [ ] Implement ReplicationService gRPC server
- [ ] Add WriteReplica/ReadReplica/DeleteReplica RPC handlers
- [ ] Create ReplicationCoordinator for quorum writes

Tests (from phase2.md):
- [ ] Unit tests for ReplicationCoordinator quorum logic
- [ ] Unit tests for replica placement strategy

### Priority 4: ReplicatedBackend

From phase2.md:
- [ ] Create ReplicatedBackend implementation
- [ ] Update API handlers to use StorageBackend trait

Tests (from phase2.md):
- [ ] Integration test: Write with node failure (quorum still met)
- [ ] Integration test: Write with quorum failure
- [ ] Integration test: Read-after-write consistency

### Priority 5: Cluster Membership

From phase2.md (depends on Priority 2):
- [ ] Implement cluster membership management (add/remove nodes)
- [ ] Handle Raft configuration changes

Tests (from phase2.md):
- [ ] Integration test: Snapshot transfer to new node
- [ ] Integration test: Network partition (split-brain prevention)
- [ ] Integration test: Concurrent writes to same object (distributed locking)

