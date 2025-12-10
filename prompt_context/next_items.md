# Next items for phase 2

## Priority 1: Testing

Integration & load tests (require multi-node setup):
- [ ] Integration test: Write with node failure (quorum still met)
- [ ] Integration test: Write with quorum failure
- [ ] Integration test: Read-after-write consistency
- [ ] Integration test: Strong consistency reads during network delay
- [ ] Chaos test: Random node failures during write workload
- [ ] Chaos test: Network partition during multipart upload
- [ ] Load test: Multi-node cluster with replication overhead
- [ ] Performance test: Replication latency P50/P90/P99

---

## Priority 2: Observability Dashboards & Alerts

- [ ] Update Grafana dashboard with cluster panels
- [ ] Add alerting rules for cluster degradation
- [ ] Create replication lag alerts
- [ ] Add quorum failure alerts

---

## Priority 3: Deployment & Operations Documentation

- [ ] Update `save.toml.example` with cluster configuration examples
- [ ] Create cluster deployment documentation (3-node, 5-node setups)
- [ ] Add TLS certificate generation guide for mTLS
- [ ] Document cluster bootstrap procedure

---

## Completed

### Internal APIs (Done)
- [x] Add internal gRPC server startup (port 8082) - `save-api/src/internal_api/`
- [x] Implement gRPC middleware for request logging (via metrics)
- [x] Add gRPC interceptors for authentication (mTLS validation)
- [x] Create internal API for cluster management operations
- [x] Add Raft-specific endpoints (add node, remove node, leader transfer stub)
- [x] Implement node drain API for graceful shutdown
- [x] Add debug endpoints for cluster state inspection

Note: Leader transfer is stubbed - full implementation requires exposing openraft's transfer API
