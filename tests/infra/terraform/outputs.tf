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

output "storage_backend" {
  description = "Storage backend (local or hetzner-volume)"
  value       = var.storage_type == "volume" ? "hetzner-volume" : "local-nvme"
}

output "volume_id" {
  description = "Hetzner volume ID (if using volume storage)"
  value       = var.storage_type == "volume" ? hcloud_volume.data[0].id : null
}

output "volume_size_gb" {
  description = "Volume size in GB (if using volume storage)"
  value       = var.storage_type == "volume" ? var.volume_size_gb : null
}

output "profile" {
  description = "Load test profile used for deployment"
  value       = var.profile
}
