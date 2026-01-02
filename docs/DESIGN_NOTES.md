# Design Notes

This file captures ongoing design decisions, experiments, and ideas for future reference.  

---

## Phase 1 — Single-node S3 store

### Object Storage Layout
- Objects stored in `objects/<hex-prefix>/<object-id>`  
- Temp files for multipart uploads in `temp/parts/`  
- Atomic move/rename for PUT completion  

### Metadata (RocksDB)
- Key namespaces:
  - `bkt:{bucket}` → bucket record  
  - `obj:{bucket}/{key}` → object metadata  
  - `mpu:{bucket}/{object}:{upload_id}` → multipart part metadata  
- Values stored as protobuf or bincode  
- RocksDB for persistence, batch writes for atomicity  

### Multipart Flow
- Track each part separately in `mpu` namespace  
- Assemble parts via streaming concat to avoid memory spikes  
- Abort uploads: remove temp files and metadata entries  

### Edge Cases
- Interrupted PUTs or GETs  
- Concurrent PUTs to same object key  
- File system crash during rename/commit  
- Partial multipart uploads  

### Checksum Strategy
- Compute SHA256 on PUT for integrity  
- Store ETag in metadata  
- Optional checksum validation on GET for debug  

### Observability & Health
- `/health` endpoint  
- `/metrics` via Prometheus  
- Structured logging using `tracing`  

### Pending / Future Ideas
- Configurable checksums (MD5 vs SHA256)
- Async GC worker with throttling
- Hooks for erasure coding in Phase 3
- Optional versioning support
- Potential caching layer for read-heavy workloads

---

## Phase 2 — Distributed Replication

### Raft Consensus Layer
- Embedded openraft for metadata coordination
- Replicated RocksDB as state machine
- Leader election with configurable timeouts (100ms heartbeat, 300ms election)
- gRPC for Raft node-to-node communication

### Replication Strategy
- Push-based synchronous replication with quorum writes
- Configurable replication factor (default: 3)
- Majority quorum: `floor(N/2) + 1` nodes must ack
- Parallel streaming to N replicas via internal gRPC API

### Storage Backend Abstraction
- `StorageBackend` trait in `save-storage/src/backend.rs`
- `LocalBackend`: Single-node implementation
- `ReplicatedBackend`: Distributed storage implementation

### Cluster Configuration
- `node_id`, `seed_nodes[]` for initial cluster discovery
- Ports: HTTP (9000), Raft (9001), Replication gRPC (9002)
- Validation: replication_factor <= cluster size

### Write Path (Distributed)
1. Acquire distributed lock (bucket/key)
2. Query Raft leader for replica placement
3. Parallel stream to N nodes via gRPC
4. Wait for quorum acks
5. Commit metadata via Raft
6. Release lock

### Read Path (Distributed)
- Eventual consistency: Read from local RocksDB
- Strong consistency: Read from Raft leader (configurable)
- Prefer local replicas, proxy to remote if needed

### Two-Phase Commit (2PC) for Replication
- Phase 1 (Prepare): Stream object data to temp file on each replica
- Phase 2 (Commit): Atomic rename from temp to final path
- Rollback on quorum failure: Abort sent to all prepared replicas
- Stale prepare cleanup: Background worker cleans orphaned temp files

### Cluster Scaling
- Auto-join: New nodes automatically join via seed_nodes on startup
- Graceful leave: Nodes remove themselves from cluster on SIGTERM
- Leader departure: Leadership transfers before node removal
- Health checking: Periodic gRPC health probes to all known nodes

### Observability
- Prometheus metrics for Raft (elections, log entries, snapshots)
- Replication metrics (quorum success/failure, latency)
- gRPC metrics (request counts, stream duration)
- Cluster health metrics (node status, connected count)

### Kubernetes Deployment
- StatefulSet with headless service for stable network identity
- PodDisruptionBudget to maintain quorum during updates
- Init container or post-install job for cluster bootstrap
- mTLS certificates via cert-manager integration
