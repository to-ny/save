# Save Development Chart

This Helm chart deploys the Save object storage system along with a full observability stack for development and testing purposes.

**WARNING: This chart is for development use only. Do NOT use in production.**

## Components

- **Save** - The main S3-compatible object storage cluster
- **Prometheus** - Metrics collection and alerting
- **Grafana** - Dashboards and visualization
- **OpenObserve** - Log aggregation and search
- **Vector** - Log collection agent (DaemonSet)

## Prerequisites

- Kubernetes 1.23+
- Helm 3.0+
- PV provisioner (optional, for persistence)

## Installation

### Quick Start

```bash
# Add required Helm repositories
helm repo add prometheus-community https://prometheus-community.github.io/helm-charts
helm repo add grafana https://grafana.github.io/helm-charts
helm repo update

# Update dependencies
cd charts/save-dev
helm dependency update

# Install the chart
helm install save-dev . -n save-dev --create-namespace
```

### Custom Values

Create a custom values file:

```yaml
# my-values.yaml
save:
  replicaCount: 5

grafana:
  adminPassword: "my-secure-password"

openobserve:
  persistence:
    enabled: true
    size: 20Gi
```

Install with custom values:

```bash
helm install save-dev . -f my-values.yaml -n save-dev --create-namespace
```

## Accessing Services

### Port Forwarding

```bash
# Grafana (dashboards)
kubectl port-forward svc/save-dev-grafana 3000:3000 -n save-dev

# Prometheus (metrics)
kubectl port-forward svc/save-dev-prometheus-server 9090:80 -n save-dev

# OpenObserve (logs)
kubectl port-forward svc/save-dev-openobserve 5080:5080 -n save-dev

# Save API
kubectl port-forward svc/save-dev-save 8080:8080 -n save-dev
```

### Default Credentials

| Service     | Username            | Password |
|-------------|---------------------|----------|
| Grafana     | admin               | admin    |
| OpenObserve | admin@example.com   | admin    |

## Configuration

### Save Configuration

See the main [Save chart documentation](../save/README.md) for all available options.

```yaml
save:
  replicaCount: 3
  config:
    server:
      http_port: 8080
    cluster:
      enabled: true
      auto_join: true
```

### Prometheus Configuration

```yaml
prometheus:
  enabled: true
  server:
    persistentVolume:
      enabled: true
      size: 10Gi
```

### Grafana Configuration

```yaml
grafana:
  enabled: true
  adminPassword: "secure-password"
  persistence:
    enabled: true
    size: 5Gi
```

### OpenObserve Configuration

```yaml
openobserve:
  enabled: true
  auth:
    rootUser: "admin@example.com"
    rootPassword: "secure-password"
  persistence:
    enabled: true
    size: 50Gi
```

### Vector Configuration

```yaml
vector:
  enabled: true
  resources:
    requests:
      cpu: 100m
      memory: 128Mi
```

## Disabling Components

You can disable any observability component:

```yaml
prometheus:
  enabled: false

grafana:
  enabled: false

openobserve:
  enabled: false

vector:
  enabled: false
```

## Pre-configured Dashboards

The chart includes a pre-configured Grafana dashboard with:

- Request rate and latency metrics
- Error rates (5xx responses)
- In-flight requests and multipart uploads
- Disk usage monitoring
- RocksDB statistics (keys, memory, cache)
- GC worker performance

## Alert Rules

Prometheus is configured with alert rules for:

- High error rates (>5%)
- Slow response times (p99 > 5s/10s)
- Low disk space (<10%/<5%)
- GC worker issues
- RocksDB cache performance
- Authentication failures
- Service availability

## Uninstallation

```bash
helm uninstall save-dev -n save-dev
kubectl delete namespace save-dev
```

## Troubleshooting

### Logs not appearing in OpenObserve

1. Check Vector DaemonSet is running:
   ```bash
   kubectl get ds -n save-dev
   ```

2. Check Vector logs:
   ```bash
   kubectl logs -l app.kubernetes.io/name=vector -n save-dev
   ```

3. Verify OpenObserve is accessible:
   ```bash
   kubectl port-forward svc/save-dev-openobserve 5080:5080 -n save-dev
   curl http://localhost:5080/healthz
   ```

### Metrics not appearing in Prometheus

1. Check Prometheus is scraping Save pods:
   ```bash
   kubectl port-forward svc/save-dev-prometheus-server 9090:80 -n save-dev
   # Visit http://localhost:9090/targets
   ```

2. Verify Save pods have the correct labels:
   ```bash
   kubectl get pods -l app.kubernetes.io/name=save -n save-dev --show-labels
   ```

### Grafana dashboard not loading

1. Check datasource configuration:
   - Navigate to Configuration > Data Sources
   - Verify Prometheus URL is correct

2. Check dashboard provisioning:
   ```bash
   kubectl logs -l app.kubernetes.io/name=grafana -n save-dev
   ```
