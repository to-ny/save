Save Object Store - Load Test Server

Services:
- save-api: http://<server-ip>:9000
- prometheus: http://<server-ip>:9090
- grafana: http://<server-ip>:3000 (admin/<your-password>)
- loki: http://<server-ip>:3100

Logs:
  # View logs via Grafana (recommended)
  Open http://<server-ip>:3000 and go to Explore > Loki

  # Or via command line
  docker-compose logs -f save-api

Restart:
  docker-compose restart save-api
