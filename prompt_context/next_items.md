# Next Items - Phase 2

## Current Status

Priority 1 completed. RocksDB storage implementation done:
- Column families added: raft_log, raft_state, raft_snapshot
- Log persistence implemented (append, read, truncate, purge)
- State persistence implemented (vote, last_applied, last_log_id)
- Command application implemented (executes bucket/object operations)
- Using serde_json for serialization (OpenRaft types with serde feature)

From `tasks/phase2.md`, remaining tasks:

## Priority 2: Raft Configuration

Tasks from phase2.md:
- Add Raft configuration from cluster config (heartbeat, election timeout)

Actual work needed:
- Wire ClusterConfig to RaftNode.new()
- Convert cluster config to OpenRaft Config
- Initialize with peer list

## Priority 3: Snapshots

Tasks from phase2.md:
- Implement Raft snapshot generation for state machine
- Add snapshot transfer mechanism for new/recovering nodes

Actual work needed:
- Implement snapshot.rs (RocksDB checkpoint, tar.gz packaging)
- Implement install_snapshot in storage.rs

## Priority 4: Network Layer

Tasks from phase2.md:
- Implement Raft network layer (node-to-node communication)

Actual work needed:
- Define Raft RPC proto (AppendEntries, Vote, InstallSnapshot)
- Implement gRPC server in network.rs
- Implement gRPC client in network.rs

## Deferred

Tasks from phase2.md (defer until core Raft works):
- Implement cluster membership management (add/remove nodes)
- Add leader election monitoring and status tracking
- Handle Raft configuration changes (dynamic cluster membership)
