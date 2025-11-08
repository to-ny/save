# Phase 1 Tasks — Single-node S3-compatible store

## Goal
Implement a local, single-node S3-compatible object storage system using RocksDB and the filesystem. Ensure correctness, durability, and basic S3 API conformance.

---

## Crate Setup
- [x] Create `save-api` crate (Axum-based REST API)
- [x] Create `save-storage` crate (object persistence)
- [x] Create `save-metadata` crate (RocksDB layer)
- [x] Create `save-common` crate (shared types, errors, config)
- [x] Add `Cargo.toml` workspace configuration

---

## Core HTTP API
- [x] Implement PUT /{bucket}/{object} with streaming
- [x] Implement GET /{bucket}/{object} with streaming
- [x] Implement DELETE /{bucket}/{object}
- [x] Implement HEAD /{bucket}/{object} for metadata
- [x] Implement multipart endpoints: initiate, upload part, complete, abort
- [x] Health and metrics endpoints: `/health`, `/metrics`
- [x] Implement PUT /{bucket} — create bucket
- [x] Implement DELETE /{bucket} — delete bucket (only if empty)
- [x] Implement GET / — list all buckets
- [x] Implement GET /{bucket} — list objects in a bucket
  - [x] Support query parameters: `prefix`, `marker`, `max-keys`
  - [x] Return object keys, sizes, ETags, last modified timestamps
- [x] Implement GET /{bucket}?uploads — list ongoing multipart uploads
- [ ] S3 XML error responses (replace current JSON format for S3 spec compliance)

---

## Storage & Metadata
- [x] FS layout with content-addressable storage: `objects/<prefix>/<sha256>`
- [x] Temp directory for staging: `temp/<id>.tmp`
- [x] RocksDB metadata with key prefixes: `bkt:`, `obj:`, `mpu:`
- [x] Atomic PUT using temp file + rename pattern
- [x] Multipart state machine: track parts, assemble on complete
- [x] Object key validation (path traversal, null bytes, length)
- [x] Bucket name validation (S3-compatible rules)
- [x] Streaming I/O with no in-memory buffering of large objects
- [x] ETag calculation via SHA256 during streaming
- [x] Atomic metadata+storage writes using RocksDB WriteBatch (Location: `save-metadata/src/lib.rs`, `save-api/src/handlers/objects/put.rs`)
- [ ] Garbage collector for temp files as background worker (Suggested: `save-storage/src/gc.rs`)
- [x] Durability guarantees with `fsync()` after critical writes (Location: `save-storage/src/lib.rs:60`)
- [ ] Multipart part file cleanup on abort (Location: `save-api/src/handlers/multipart/abort.rs`)
- [ ] Object versioning support with version IDs and API endpoints (GET/DELETE ?versionId)

---

## Security & Authentication
- [x] Basic auth middleware extracts access key from Authorization header
- [x] Compares access key to config
- [ ] Full SigV4 signature validation (Location: `save-api/src/auth.rs`)
- [ ] Request expiration checking via `X-Amz-Date` header (±15 min window)
- [ ] Secure credential storage (no logging of secret keys)
- [ ] Audit logging for security events

---

## Concurrency & Error Handling
- [x] Async I/O with tokio for concurrent request handling
- [x] Request-level isolation (each request is independent)
- [x] Proper error propagation with `anyhow` and `thiserror`
- [ ] Concurrent PUT protection (Location: `save-api/src/handlers/objects/put.rs`)
- [ ] Request rate limiting via `tower` middleware
- [ ] Connection limits to avoid file descriptor exhaustion
- [ ] Graceful shutdown with request draining
- [ ] Backpressure to handle storage overwhelm (return 503 when overloaded)

---

## Observability
- [x] Structured logging with `tracing` crate
- [x] JSON log output for production
- [x] Request instrumentation with `#[instrument]` macros
- [x] Prometheus metrics endpoint at `/metrics`
- [x] Metrics: `save_http_requests_total` (counter by endpoint/method/status)
- [x] Metrics: `save_http_request_duration_seconds` (histogram)
- [x] Metrics: `save_object_size_bytes` (histogram)
- [x] Metrics: `save_multipart_uploads_in_progress` (gauge)
- [x] Health check endpoint at `/health`
- [x] Middleware for automatic request tracking
- [ ] Request tracing with correlation IDs
- [ ] Enhanced metrics (disk usage, RocksDB stats, error rates, temp files, connections)
- [ ] Readiness probe at `/health/ready` with deep health checks
- [ ] Grafana dashboard template in `docs/grafana/`

---

## Testing
- [x] Storage layer unit tests: PUT/GET/DELETE, large files, invalid keys (5 tests)
- [x] Metadata layer unit tests: Buckets (7 tests), Objects (6 tests), Multipart (11 tests)
- [x] API auth unit tests: Access key extraction (2 tests)
- [x] Metrics unit tests: Encoding, recording (4 tests)
- [x] Object operations integration tests: 40 tests covering PUT/GET/DELETE/HEAD/LIST
- [x] Bucket operations integration tests: 19 tests covering create/delete/list
- [x] Multipart integration tests: 13 tests covering complete flow, abort, errors
- [x] Metrics integration tests: 5 tests for endpoint tracking
- [ ] Concurrent operation tests (simultaneous PUTs, concurrent GET during PUT, parallel multipart uploads)
- [ ] Crash recovery tests (kill during PUT/multipart, verify metadata consistency)
- [ ] Edge case tests (filesystem full, RocksDB corruption, partial multipart, timeouts)
- [ ] AWS SDK compatibility tests (verify SDK can upload/download, multipart via SDK)
- [ ] Performance benchmarks using `criterion` (throughput, latency percentiles)
- [ ] Load tests using `wrk` or `k6` (1000 req/sec sustained, leak detection)

---

## Dev Environment & Deployment
- [x] Configuration system with TOML parsing
- [x] Environment variable: `SAVE_CONFIG` (defaults to `save.toml`)
- [x] Default config in code if file missing
- [x] Config validation on load
- [x] Documentation: README per crate, ARCHITECTURE.md, ROADMAP.md
- [ ] `save.toml.example` template with documented options and production-ready defaults
- [ ] Dockerfile with multi-stage build (distroless/alpine base, health checks, port 9000, volume mounts)
- [ ] `docker-compose.yml` for local development (optional Prometheus + Grafana)
- [ ] Makefile or Justfile for common tasks (build, test, run, docker, clean)
- [ ] Setup/teardown scripts in `scripts/` (setup.sh, teardown.sh, seed-data.sh)
- [ ] Deployment documentation (systemd service, TLS termination, RocksDB backup, runbooks)
- [ ] CLI tool `save-cli` for administration (list objects, verify metadata, trigger GC, export metrics)
