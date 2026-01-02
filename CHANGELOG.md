# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Stale 2PC prepare cleanup worker with Prometheus metrics
- Kubernetes-native deployment documentation (ADR-007)

### Changed
- Readiness check log level changed to debug
- Improved documentation for current project state

### Fixed
- Panic safety improvements in middleware and storage backends

## [0.1.0] - 2024-12-01

### Added

#### Phase 1: Single-Node S3-Compatible Storage
- Full S3 REST API (buckets, objects, multipart uploads)
- AWS Signature Version 4 (SigV4) authentication
- RocksDB metadata storage with atomic operations
- Content-addressable object storage with SHA256 checksums
- Streaming I/O for large objects (no memory buffering)
- Multipart upload support with part tracking and assembly
- Garbage collector for orphaned temp files
- Prometheus metrics endpoint (`/metrics`)
- Health and readiness endpoints (`/health`, `/health/ready`)
- Structured JSON logging with tracing
- TOML configuration system
- CLI administration tool (`save-cli`)
- Docker multi-stage build with cargo-chef caching
- Grafana dashboards and Prometheus alerting rules

#### Phase 2: Distributed Replication (In Progress)
- Raft consensus using OpenRaft for metadata coordination
- Quorum-based replication with configurable replication factor
- Two-phase commit (2PC) for distributed writes
- gRPC replication protocol with mTLS support
- Distributed locking for concurrent write protection
- Automatic leader election and failover
- Node health monitoring with gRPC health checks
- Transparent request forwarding to Raft leader
- Auto-join for new cluster members
- Graceful leave on node shutdown
- Helm chart for Kubernetes deployment
- Development Helm chart with observability stack (Prometheus, Grafana)
- Cluster status API (`/cluster/status`)
- Replication metrics (quorum success/failure, latency)

### Security
- mTLS for inter-node communication
- SigV4 request signing validation
- Request expiration checking (15-minute window)
- No logging of secret keys

[Unreleased]: https://github.com/to-ny/save/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/to-ny/save/releases/tag/v0.1.0
