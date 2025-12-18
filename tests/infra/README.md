# Load Test Infrastructure

Kubernetes-based infrastructure for deploying Save and running load tests.

## Prerequisites

- Docker Desktop with Kubernetes enabled (or any K8s cluster)
- kubectl configured for your cluster
- helm 3.x

## Quick Start

```bash
make deploy-smoke
make build-image
make run-loadtest
```

## Deployment Profiles

| Profile | Replicas | CPU | Memory |
|---------|----------|-----|--------|
| `smoke` | 1 | 2 | 2GB |
| `medium` | 3 | 4 | 8GB |
| `large` | 3 | 8 | 16GB |

## Remote Load Testing

Uses Terraform to provision a managed Kubernetes cluster and run load tests in-cluster.

### Prerequisites

- Terraform >= 1.0
- DigitalOcean account with API token
- doctl CLI (for container registry)

### Run Remote Tests

```bash
cd terraform
export TF_VAR_do_token="dop_v1_xxx"  # or use your shell's secret manager

terraform init
terraform apply -var="test_name=test_mixed_workload" -var="save_profile=medium"

# Get kubeconfig for manual inspection
terraform output -raw kubeconfig > ~/.kube/save-loadtest.yaml
export KUBECONFIG=~/.kube/save-loadtest.yaml
kubectl logs job/save-loadtest -n save-test

# Destroy when done
terraform destroy
```

### Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `do_token` | - | DigitalOcean API token (required) |
| `cluster_name` | `save-loadtest` | K8s cluster name |
| `region` | `nyc1` | DO region |
| `node_size` | `s-4vcpu-8gb` | Worker node size |
| `node_count` | `3` | Number of workers |
| `save_profile` | `medium` | Helm values profile |
| `test_name` | `test_quick_smoke` | Load test to run |

## Configuration

Override via environment variables in the Job:

- `SAVE_ENDPOINT` - API endpoint (default: `http://save:9000`)
- `SAVE_ACCESS_KEY` / `SAVE_SECRET_KEY` - Credentials
- `SAVE_BUCKET` - Target bucket
- `TEST_FILTER` - Test filter (default: `test_quick_smoke`)

Available tests in `load_test`:
- `test_quick_smoke` - 5 second smoke test (default)
- `test_mixed_workload` - Mixed read/write
- `test_read_heavy_workload` - Read-heavy
- `test_write_heavy_workload` - Write-heavy

Run specific test:
```bash
make run-loadtest TEST=test_mixed_workload
```

## Troubleshooting

```bash
kubectl describe pod -n save-test
kubectl logs <pod-name> -n save-test
make status
```
