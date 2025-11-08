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
- [x] Implement PUT /{bucket}/{object} with SigV4 auth
- [x] Implement GET /{bucket}/{object}
- [x] Implement DELETE /{bucket}/{object}
- [ ] Implement HEAD /{bucket}/{object} for metadata
- [ ] Implement multipart endpoints: initiate, upload part, complete, abort
- [ ] Health and metrics endpoints: `/health`, `/metrics`

---

## Storage & Metadata
- [ ] FS layout: `objects/` + `temp/parts/`
- [ ] RocksDB metadata: buckets, objects, multipart uploads
- [ ] Implement atomic PUT: temp file + rename + metadata batch write
- [ ] Multipart state machine: track parts, assemble stream, handle abort
- [ ] Garbage collector for orphaned temp files and tombstones

---

## Concurrency & Error Handling
- [ ] Handle concurrent PUTs/GETs to the same object
- [ ] Recover gracefully from interrupted uploads or FS crashes
- [ ] Ensure atomic writes to RocksDB and object files

---

## Observability
- [ ] Structured logging using `tracing`
- [ ] Prometheus metrics: request count, latency, object size, multipart uploads
- [ ] Health checks and basic admin endpoints

---

## Testing
- [ ] Unit tests for metadata layer, storage layer, multipart logic
- [ ] Integration tests for S3 API using `aws-cli` or local HTTP client
- [ ] Edge case tests: interrupted PUT, concurrent PUTs, partial multipart
- [ ] Performance tests for streaming large files

---

## Dev Environment
- [ ] Dockerfile for local dev
- [ ] `docker-compose` for single-node setup
- [ ] Config template (`save.toml`) with data paths, ports, and credentials

---

## Acceptance Criteria
- [ ] All PUT/GET/DELETE flows work end-to-end
- [ ] Multipart upload completes atomically
- [ ] Metadata persists across restarts
- [ ] Integration tests pass
- [ ] Metrics and health endpoints functional
