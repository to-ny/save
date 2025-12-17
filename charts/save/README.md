# Save Helm Chart

Deploy Save, a high-performance S3-compatible distributed object store built in Rust, on Kubernetes.

## Overview

Save is a Raft-based distributed object store that provides:
- S3-compatible API
- Strong consistency via Raft consensus
- Automatic data replication
- Horizontal scalability

## Prerequisites

- Kubernetes 1.23+
- Helm 3.8+
- PV provisioner support (for persistence)
- (Optional) Prometheus Operator for metrics

## Quick Start

### Development Deployment

```bash
# Add the repo (if published)
# helm repo add save https://charts.save.io

# Install with development values
helm install save ./charts/save -f charts/save/values-development.yaml

# Or with inline credentials
helm install save ./charts/save \
  --set auth.accessKey=myaccesskey \
  --set auth.secretKey=mysecretkey \
  --set replicaCount=3
```

### Production Deployment

**Step 1: Create credentials secret**

```bash
kubectl create secret generic save-credentials \
  --from-literal=access-key=$(openssl rand -hex 16) \
  --from-literal=secret-key=$(openssl rand -hex 32)
```

**Step 2: Deploy with production values**

```bash
helm install save ./charts/save \
  -f charts/save/values-production.yaml \
  --set auth.existingSecret=save-credentials
```

## Configuration

### Key Parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `replicaCount` | Number of cluster nodes (must be odd) | `3` |
| `image.repository` | Container image repository | `ghcr.io/save/save-api` |
| `image.tag` | Container image tag | Chart appVersion |
| `auth.accessKey` | S3 access key (dev only) | `""` |
| `auth.secretKey` | S3 secret key (dev only) | `""` |
| `auth.existingSecret` | Name of existing credentials secret | `""` |
| `persistence.enabled` | Enable persistent storage | `true` |
| `persistence.dataSize` | Size of data volume | `10Gi` |
| `persistence.metadataSize` | Size of metadata volume | `5Gi` |

### Storage Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `persistence.storageClass` | Storage class (empty = default) | `""` |
| `persistence.accessModes` | PVC access modes | `["ReadWriteOnce"]` |
| `storage.fsyncMode` | Fsync mode: none, data, all | `"data"` |

### Cluster Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `cluster.replicationFactor` | Data replication factor | `3` |
| `tls.enabled` | Enable mTLS for inter-node traffic | `false` |
| `tls.existingSecret` | TLS secret with crt, key, ca.crt | `""` |

### Performance Tuning

| Parameter | Description | Default |
|-----------|-------------|---------|
| `server.workerThreads` | HTTP server worker threads | `4` |
| `server.maxBlockingThreads` | Max blocking threads | `512` |
| `metadata.writeBufferSizeMb` | RocksDB write buffer (MB) | `128` |
| `metadata.blockCacheSizeMb` | RocksDB block cache (MB) | `256` |

### Resource Management

| Parameter | Description | Default |
|-----------|-------------|---------|
| `resources.requests.memory` | Memory request | `512Mi` |
| `resources.requests.cpu` | CPU request | `500m` |
| `resources.limits.memory` | Memory limit | `2Gi` |
| `resources.limits.cpu` | CPU limit | `2` |

### Observability

| Parameter | Description | Default |
|-----------|-------------|---------|
| `metrics.enabled` | Enable /metrics endpoint | `true` |
| `metrics.serviceMonitor.enabled` | Create ServiceMonitor | `false` |
| `metrics.podMonitor.enabled` | Create PodMonitor | `false` |
| `prometheusRule.enabled` | Create PrometheusRule | `false` |

### High Availability

| Parameter | Description | Default |
|-----------|-------------|---------|
| `podDisruptionBudget.enabled` | Enable PDB | `true` |
| `podDisruptionBudget.maxUnavailable` | Max unavailable pods | `1` |
| `affinity` | Pod affinity rules | Anti-affinity by hostname |
| `topologySpreadConstraints` | Topology spread rules | `[]` |

## Architecture

```
                    ┌─────────────────┐
                    │   LoadBalancer  │  (optional)
                    │   or Ingress    │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │  ClusterIP Svc  │
                    │   (save:9000)   │
                    └────────┬────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
   ┌────▼────┐         ┌────▼────┐         ┌────▼────┐
   │ save-0  │◄───────►│ save-1  │◄───────►│ save-2  │
   │  :9000  │  Raft   │  :9000  │  Raft   │  :9000  │
   │  :9001  │  :9001  │  :9001  │  :9001  │  :9001  │
   │  :9002  │ Repl    │  :9002  │ Repl    │  :9002  │
   └────┬────┘  :9002  └────┬────┘  :9002  └────┬────┘
        │                    │                    │
   ┌────▼────┐         ┌────▼────┐         ┌────▼────┐
   │  PVC    │         │  PVC    │         │  PVC    │
   │ (data)  │         │ (data)  │         │ (data)  │
   │(metadata)         │(metadata)         │(metadata)
   └─────────┘         └─────────┘         └─────────┘
```

**Ports:**
- `9000`: S3-compatible HTTP API
- `9001`: Raft consensus (gRPC)
- `9002`: Data replication (gRPC)

## Operations

### Checking Cluster Status

```bash
# View all pods
kubectl get pods -l app.kubernetes.io/name=save

# Check cluster status via API
kubectl exec -it save-0 -- curl -s http://localhost:9000/cluster/status | jq

# Check individual node health
kubectl exec -it save-0 -- curl -s http://localhost:9000/health
```

### Scaling the Cluster

**Important:** Only scale to odd numbers (3, 5, 7) for Raft quorum.

```bash
# Scale up (requires re-initialization for new nodes)
helm upgrade save ./charts/save --set replicaCount=5 --reuse-values

# Note: After scaling, you may need to add new nodes to the cluster via API
```

### Upgrading

```bash
# Standard upgrade
helm upgrade save ./charts/save -f values-production.yaml

# Upgrade with new image
helm upgrade save ./charts/save --set image.tag=v1.2.0 --reuse-values
```

The StatefulSet uses `RollingUpdate` strategy with `maxUnavailable: 1` by default.

### Backup and Restore

**Creating a backup:**

```bash
# Backup metadata (RocksDB)
kubectl exec save-0 -- tar czf - /var/lib/save/metadata > metadata-backup.tar.gz

# Backup data (for each node)
for i in 0 1 2; do
  kubectl exec save-$i -- tar czf - /var/lib/save/data > data-backup-$i.tar.gz
done
```

**Restoring from backup:**

1. Scale down the StatefulSet to 0
2. Delete existing PVCs (if needed)
3. Create new PVCs and restore data
4. Scale up and re-initialize cluster

### Disaster Recovery

If quorum is lost (majority of nodes down):

1. Identify the node with the most recent data
2. Use that node's data to bootstrap a new single-node cluster
3. Add additional nodes one at a time

### Cluster Maintenance

**Triggering Leadership Election**

If the current Raft leader is unresponsive or you need to move leadership to another node (e.g., before maintenance), you can trigger an election:

```bash
# Trigger election on a specific node (it will campaign to become leader)
kubectl exec -it save-1 -- curl -X POST http://localhost:9000/cluster/trigger-elect

# Verify new leader
kubectl exec -it save-0 -- curl -s http://localhost:9000/cluster/status | jq '.leader_id'
```

**Note:** The node receiving the trigger-elect request will attempt to become the new leader. This requires the node to be a voter in the cluster and have up-to-date logs.

## Monitoring

### Prometheus Integration

Enable ServiceMonitor for Prometheus Operator:

```bash
helm upgrade save ./charts/save \
  --set metrics.serviceMonitor.enabled=true \
  --set metrics.serviceMonitor.labels.release=prometheus
```

### Key Metrics

| Metric | Description |
|--------|-------------|
| `save_http_requests_total` | Total HTTP requests by method, path, status |
| `save_http_request_duration_seconds` | Request latency histogram |
| `save_in_flight_requests` | Current in-flight requests |
| `save_disk_usage_bytes` | Disk usage by type (total, used, available) |
| `save_rocksdb_stats` | RocksDB statistics |

### Alerting

Enable default alerting rules:

```bash
helm upgrade save ./charts/save \
  --set prometheusRule.enabled=true \
  --set prometheusRule.labels.release=prometheus
```

## Security

### Production Checklist

- [ ] Use `auth.existingSecret` instead of inline credentials
- [ ] Enable `networkPolicy.enabled` for network isolation
- [ ] Enable `tls.enabled` for inter-node encryption
- [ ] Configure `podSecurityContext` and `securityContext`
- [ ] Use a non-default `storageClass` with encryption at rest
- [ ] Enable Ingress TLS for external access

### Network Policy

Enable network isolation:

```bash
helm upgrade save ./charts/save --set networkPolicy.enabled=true
```

This restricts:
- Ingress to S3 API (9000) from specified sources
- Raft/replication ports (9001, 9002) to cluster members only
- Egress to DNS and cluster members only

### mTLS for Inter-Node Communication

```bash
# Create TLS secret
kubectl create secret generic save-tls \
  --from-file=tls.crt=server.crt \
  --from-file=tls.key=server.key \
  --from-file=ca.crt=ca.crt

# Enable mTLS
helm upgrade save ./charts/save \
  --set tls.enabled=true \
  --set tls.existingSecret=save-tls
```

## Troubleshooting

### Cluster Won't Initialize

1. Check all pods are running: `kubectl get pods`
2. Check init job logs: `kubectl logs -l app.kubernetes.io/component=cluster-init`
3. Verify DNS resolution between pods
4. Check if ports 9001/9002 are accessible between pods

### Slow Performance

1. Check resource utilization: `kubectl top pods`
2. Increase RocksDB cache: `--set metadata.blockCacheSizeMb=512`
3. Increase worker threads: `--set server.workerThreads=8`
4. Check storage I/O performance

### Pod CrashLooping

1. Check logs: `kubectl logs save-0 --previous`
2. Verify PVC is bound: `kubectl get pvc`
3. Check resource limits aren't too restrictive
4. Verify credentials secret exists

### Split Brain / Quorum Issues

1. Check cluster status on each node
2. Ensure network connectivity between all nodes
3. If quorum is lost, see Disaster Recovery section

## Values Files

The chart includes pre-configured values files:

- `values.yaml` - Default values with documentation
- `values-production.yaml` - Production-ready settings
- `values-development.yaml` - Minimal resources for local dev

## License

Apache License 2.0
