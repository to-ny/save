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

From phase2.md (blocking membership changes):
- [ ] Implement Raft snapshot generation for state machine
- [ ] Add snapshot transfer mechanism for new/recovering nodes

Work needed:
- Implement `snapshot.rs` (RocksDB checkpoint, packaging)
- Implement `install_snapshot` in storage.rs
- Wire streaming snapshot RPC in server.rs

### Priority 2: Leader Election Monitoring

From phase2.md:
- [ ] Add leader election monitoring and status tracking

Work needed:
- Expose Raft metrics (current leader, term, role)
- Add `/cluster/status` endpoint

### Priority 3: Replication Service

From phase2.md (can be parallel with Priority 1-2):
- [ ] Implement ReplicationService gRPC server
- [ ] Add WriteReplica/ReadReplica/DeleteReplica RPC handlers
- [ ] Create ReplicationCoordinator for quorum writes

### Priority 4: ReplicatedBackend

From phase2.md (depends on Priority 3):
- [ ] Create ReplicatedBackend implementation
- [ ] Update API handlers to use StorageBackend trait

### Priority 5: Cluster Membership

From phase2.md (depends on Priority 1):
- [ ] Implement cluster membership management (add/remove nodes)
- [ ] Handle Raft configuration changes

---

## Testing Milestones

After Priority 2:
- Integration test: 3-node cluster formation
- Integration test: Leader election after leader crash

After Priority 4:
- Integration test: Write with node failure (quorum still met)
- Integration test: Read-after-write consistency
