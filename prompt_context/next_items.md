# Next items for phase 2

## Priority 1: Testing - COMPLETE
- [x] Integration test: Write with node failure (quorum still met)
- [x] Integration test: Write with quorum failure
- [x] Integration test: Read-after-write consistency
- [x] Integration test: Strong consistency reads during network delay
- [x] Chaos test: Random node failures during write workload (follower only)
- [x] Chaos test: Network partition during multipart upload
- [x] Integration test: Leader election after leader crash
- [x] Integration test: Node recovery and catch-up
- [x] Integration test: Network partition (split-brain prevention)
- [x] Integration test: Snapshot transfer to new node

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
