# Docker Setup

- `docker-compose.yml` - Single-node development
- `docker-compose.cluster.yml` - 3-node cluster with load balancer

## Single-Node

```bash
docker-compose up -d save-api

# With observability
docker-compose --profile observability up -d
```

## Cluster (3 nodes)

```bash
docker-compose -f docker-compose.cluster.yml up -d

# With observability
docker-compose -f docker-compose.cluster.yml --profile observability up -d
```

The cluster auto-initializes once all nodes are healthy.

```bash
# Verify cluster
curl http://localhost:9000/cluster/status

# Clean shutdown
docker-compose -f docker-compose.cluster.yml down -v
```

## Endpoints

- S3 API: http://localhost:9000
- HAProxy stats (cluster only): http://localhost:8404/stats
- Grafana: http://localhost:3001 (admin/admin)
- Prometheus: http://localhost:9090
- OpenObserve: http://localhost:5080 (admin@example.com/admin)
