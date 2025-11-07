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
