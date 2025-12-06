# Next Items - Phase 2

## Current Status

**Raft Consensus Layer** - Core implementation complete:
- RocksDB storage with column families (raft_log, raft_state, raft_snapshot)
- Log persistence, state persistence, command application
- gRPC client/server for Raft RPCs (proto/raft.proto)
- Network layer (RaftNetworkFactory trait)
- ClusterConfig integration

**Replication Infrastructure** - Proto defined, implementation pending:
- proto/replication.proto exists
- No service implementation yet

**Storage Backend** - Trait abstraction complete:
- StorageBackend trait and LocalBackend wrapper done
- ReplicatedBackend not started

---

## Next Priorities

### Priority 1: Raft Snapshots

From phase2.md:
- [ ] Implement Raft snapshot generation for state machine
- [ ] Add snapshot transfer mechanism for new/recovering nodes

Work:
- Implement `snapshot.rs` (RocksDB checkpoint, packaging)
- Wire streaming snapshot RPC in server.rs

Tests (from phase2.md):
- [ ] Integration test: Snapshot transfer to new node

### Priority 2: Leader Election Monitoring + Cluster Tests

From phase2.md:
- [ ] Add leader election monitoring and status tracking

Work:
- Expose Raft metrics (current leader, term, role)
- Add `/cluster/status` endpoint
- Add `ClusterEnv` to `tests/crash-recovery/tests/common/`

Tests (from phase2.md):
- [ ] Integration test: 3-node cluster formation
- [ ] Integration test: Leader election after leader crash
- [ ] Integration test: Node recovery and catch-up

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

From phase2.md (depends on Priority 1):
- [ ] Implement cluster membership management (add/remove nodes)
- [ ] Handle Raft configuration changes

Tests (from phase2.md):
- [ ] Integration test: Network partition (split-brain prevention)
- [ ] Integration test: Concurrent writes to same object (distributed locking)

