version: '3.8'
services:
  save-api:
    image: save-api:latest
    container_name: save-api
    ports:
      - "9000:9000"
    volumes:
      - save-data:/var/lib/save/data
      - save-metadata:/var/lib/save/metadata
      - ./save.toml:/app/save.toml:ro
    environment:
      - RUST_LOG=info
      - LOG_FORMAT=json
      - SAVE_CONFIG=/app/save.toml
    restart: unless-stopped
    logging:
      driver: json-file
      options:
        max-size: "10m"
        max-file: "3"
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:9000/health"]
      interval: 10s
      timeout: 5s
      retries: 5

  prometheus:
    image: prom/prometheus:${prometheus_version}
    container_name: prometheus
    ports:
      - "9090:9090"
    volumes:
      - prometheus-data:/prometheus
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
    restart: unless-stopped

  loki:
    image: grafana/loki:${loki_version}
    container_name: loki
    ports:
      - "3100:3100"
    volumes:
      - ./loki-config.yml:/etc/loki/local-config.yaml:ro
      - loki-data:/loki
    command: -config.file=/etc/loki/local-config.yaml
    restart: unless-stopped

  promtail:
    image: grafana/promtail:${promtail_version}
    container_name: promtail
    volumes:
      - ./promtail-config.yml:/etc/promtail/config.yml:ro
      - /var/lib/docker/containers:/var/lib/docker/containers:ro
      - /var/run/docker.sock:/var/run/docker.sock:ro
    command: -config.file=/etc/promtail/config.yml
    restart: unless-stopped
    depends_on:
      - loki

  grafana:
    image: grafana/grafana:${grafana_version}
    container_name: grafana
    ports:
      - "3000:3000"
    volumes:
      - grafana-data:/var/lib/grafana
      - ./grafana-datasources.yml:/etc/grafana/provisioning/datasources/datasources.yml:ro
    environment:
      - GF_SECURITY_ADMIN_USER=admin
      - GF_SECURITY_ADMIN_PASSWORD=${grafana_password}
      - GF_USERS_ALLOW_SIGN_UP=false
      - GF_AUTH_ANONYMOUS_ENABLED=false
    restart: unless-stopped
    depends_on:
      - prometheus
      - loki

volumes:
  save-data:
  save-metadata:
  prometheus-data:
  loki-data:
  grafana-data:
