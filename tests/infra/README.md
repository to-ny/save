# Save Load Test Infrastructure

Automated provisioning of remote servers for load testing using Terraform and Hetzner Cloud.

## Prerequisites

- Terraform >= 1.0
- Docker
- Hetzner Cloud account and API token
- SSH key pair (`~/.ssh/id_rsa.pub`)
- AWS CLI (optional, for smoke tests)

## Quick Start

```bash
cd tests/infra
cp .env.template .env    # Edit with your credentials
source .env
make check
make deploy-medium
source .env.loadtest
cargo test -p save-loadtest --features load_tests
make teardown
```

## Setup

Get Hetzner Cloud API token from https://console.hetzner.cloud/ → Security → API tokens and add to `.env`:

```bash
export HCLOUD_TOKEN=your_token
export SAVE_ACCESS_KEY=your_access_key
export SAVE_SECRET_KEY=your_secret_key
export GRAFANA_PASSWORD=your_password  # Optional, defaults to "changeme"
```

Ensure SSH key exists: `ls ~/.ssh/id_rsa.pub || ssh-keygen -t rsa -b 4096`

## Profiles

| Profile | vCPU | RAM | Disk | Cost/mo | Use Case |
|---------|------|-----|------|---------|----------|
| smoke   | 2    | 2GB | 40GB | €4.15   | Quick validation |
| medium  | 4    | 8GB | 160GB | €13.90 | Realistic workloads |
| large   | 8    | 16GB | 240GB | €26.90 | Stress testing |

## Project Structure

```
tests/infra/
├── .env.template
├── Makefile                   # Main interface
├── profiles.toml
├── scripts/                   # Implementation (called by Makefile)
│   ├── common.sh
│   ├── deploy.sh
│   ├── update.sh
│   └── ...
└── terraform/
    ├── main.tf
    ├── variables.tf
    ├── profiles/
    └── files/                 # Config templates
```

## Security

**IP Allowlisting**: Deployment auto-detects your public IP and restricts firewall access (SSH, save-api, Prometheus, Grafana). Add more IPs: `export ALLOWED_SOURCE_IPS="203.0.113.0/24"`

**Credentials**: No hardcoded passwords. All credentials via environment variables, marked sensitive in Terraform.

**Profile-Aware Config**: Each profile auto-configures optimal worker threads, buffer sizes, and cache sizes.

**Pinned Versions**: Prometheus v2.54.1, Grafana 11.3.0, Loki 3.0.0, Promtail 3.0.0

## Usage

### Deploy

```bash
make deploy-medium  # Default
make deploy-smoke   # Quick validation
make deploy-large   # Stress testing
```

The script provisions infrastructure, builds/uploads Docker image, runs smoke tests, and outputs endpoint URL.

### Update

After code changes, update save-api without full redeploy:

```bash
make update
```

Automatically backs up current image and rolls back if health check fails.

### Load Tests

```bash
source tests/infra/.env.loadtest
cargo test -p save-loadtest --features load_tests
```

See [load tests' README](../loadtest/README.md) for more information.

### Monitoring

**Grafana**: `http://<server-ip>:3000` (admin/changeme)
- Metrics from Prometheus
- Logs from Loki
- Query example: `{service="save-api"} |= "error"`

**Direct access**:
- Prometheus: `http://<server-ip>:9090`
- save-api: `http://<server-ip>:9000`

### Logs

```bash
make logs          # Recent logs
make logs-follow   # Follow in real-time
```

### Teardown

```bash
make teardown
```

## Troubleshooting

**Diagnose issues**:
```bash
make diagnose
```

Shows SSH connectivity, Docker status, containers, logs, health endpoints, firewall rules, disk/memory/CPU usage.

**Orphaned resources**:
```bash
make cleanup         # List
make cleanup-delete  # Delete (requires jq)
```

**Manual access**:
```bash
ssh root@<server-ip>
cd /opt/save && docker-compose logs -f save-api
docker-compose restart save-api
curl http://localhost:9000/health
```

**Health check from outside**:
```bash
curl http://<server-ip>:9000/health
```

If issues persist: `make teardown && make deploy-medium`
