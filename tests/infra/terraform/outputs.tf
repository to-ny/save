output "server_ip" {
  description = "Public IP address of the server"
  value       = hcloud_server.save.ipv4_address
}

output "server_name" {
  description = "Server hostname"
  value       = hcloud_server.save.name
}

output "ssh_connection" {
  description = "SSH connection string"
  value       = "ssh root@${hcloud_server.save.ipv4_address}"
}

output "save_endpoint" {
  description = "Save API endpoint URL"
  value       = "http://${hcloud_server.save.ipv4_address}:9000"
}

output "prometheus_endpoint" {
  description = "Prometheus endpoint URL"
  value       = "http://${hcloud_server.save.ipv4_address}:9090"
}

output "grafana_endpoint" {
  description = "Grafana dashboard URL (admin/changeme)"
  value       = "http://${hcloud_server.save.ipv4_address}:3000"
}

output "loki_endpoint" {
  description = "Loki log aggregation endpoint"
  value       = "http://${hcloud_server.save.ipv4_address}:3100"
}
