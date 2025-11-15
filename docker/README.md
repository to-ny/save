# Docker Setup

Observability stack: Save + Prometheus + Grafana + Alertmanager

## Quick Start

```bash
docker-compose up -d
```

Services:
- Save: http://localhost:9000
- Grafana: http://localhost:3001 (admin/admin)
- Prometheus: http://localhost:9090
- Alertmanager: http://localhost:9093

## Configuration

- Save: `docker/save/save.toml`
- Prometheus: `docker/prometheus/prometheus.yml`
- Alerts: `docker/prometheus/alerts.yml`
- Dashboard: `docker/grafana/dashboards/save-dashboard.json`
