# Phase 2: First Tasks to Implement

Recommended starting tasks from `tasks/phase2.md`.

---

## Priority 1: Foundational Dependencies

### Raft Consensus Layer
```
- [ ] Add cluster configuration to SaveConfig (Location: save-common/src/config.rs)
- [ ] Add openraft dependency to workspace Cargo.toml
```

**Dependencies needed**:
- `openraft`
- `tonic` (gRPC)
- `prost` (protobuf)
- `tonic-build` (build dependency)

---

## Priority 2: Independent Work

### Storage Backend
```
- [ ] Create StorageBackend trait abstraction (Location: save-storage/src/backend.rs)
- [ ] Implement LocalBackend wrapper (Location: save-storage/src/local_backend.rs)
```

### Replication Infrastructure
```
- [ ] Create gRPC service definition proto/replication.proto
- [ ] Generate Rust code from protobuf (tonic-build in build.rs)
```

**gRPC services**: WriteReplica, ReadReplica, DeleteReplica, ReplicationHealth

---

## Priority 3: Core Raft Implementation

**Requires**: Priority 1 complete

```
- [ ] Create Raft node module in save-metadata crate
- [ ] Implement RaftStateMachine trait wrapping RocksDB
- [ ] Add Raft log storage using RocksDB column family
```

---

## Key Resources

- docs/adrs/006-distributed-metadata-strategy.md
- docs/REPLICATION_PROTOCOL.md
- docs/ARCHITECTURE.md (Phase 2 section)
- tasks/phase2.md
- https://github.com/datafuselabs/openraft/tree/main/examples

---

## Notes

- Test incrementally
