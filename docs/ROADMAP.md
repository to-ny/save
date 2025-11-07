# Roadmap

This file tracks implementation goals across major phases.  
It focuses on functional milestones and completion criteria, not architecture.

---

## Phase 1 — Single Node (MVP)

**Goal:** Local S3-compatible object store using RocksDB and filesystem storage.

**Deliverables**
- HTTP API (Axum) with SigV4 auth  
- Object PUT/GET/DELETE  
- Multipart uploads  
- Metadata in RocksDB  
- Object files on local FS  
- Basic health and metrics endpoints  
- Integration tests with AWS CLI  

**Exit criteria**
- Data persists across restarts  
- All API paths pass local conformance tests  
- End-to-end upload and retrieval validated

---

## Phase 2 — Cluster Coordination

**Goal:** Multi-node operation with consistent metadata and replication.

**Deliverables**
- Raft or etcd-based cluster state  
- Object placement and replication strategy  
- Node join/leave and rebalance handling  
- Background sync and consistency checks  

**Exit criteria**
- Automatic recovery after node restart  
- Data consistency verified after failover

---

## Phase 3 — Erasure Coding & Healing

**Goal:** Efficient, durable storage with redundancy.

**Deliverables**
- Erasure coding for object shards  
- Background healing for missing/corrupt parts  
- Rebalancer for capacity changes  

**Exit criteria**
- Shard loss recoverable without data loss  
- Rebalance runs without interrupting clients

---

## Phase 4 — IAM, Policies, and Operations

**Goal:** Production readiness and manageability.

**Deliverables**
- IAM users and bucket policies  
- Encryption at rest  
- Lifecycle rules and versioning  
- Prometheus metrics and CLI tools  
- Operator endpoints (health, diagnostics, GC)

**Exit criteria**
- Security, lifecycle, and observability features validated

---

## Phase 5 — Extended Integrations (optional)

**Goal:** Interoperability and ecosystem maturity.

**Deliverables**
- SDKs or gateway mode for other clouds  
- Object locking and compliance features  
- Pluggable storage backends  

---

_This roadmap evolves as design details mature; it guides milestone planning, not implementation specifics._
