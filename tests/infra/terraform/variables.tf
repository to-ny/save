variable "hcloud_token" {
  description = "Hetzner Cloud API token"
  type        = string
  sensitive   = true
}

variable "ssh_public_key" {
  description = "SSH public key for server access"
  type        = string
}

variable "profile" {
  description = "Load test profile name (smoke, medium, large)"
  type        = string
  default     = "medium"
}

variable "server_type" {
  description = "Hetzner server type"
  type        = string
}

variable "location" {
  description = "Hetzner datacenter location"
  type        = string
  default     = "nbg1"
}

# Security credentials
variable "save_access_key" {
  description = "S3 access key for save-api authentication"
  type        = string
  sensitive   = true
}

variable "save_secret_key" {
  description = "S3 secret key for save-api authentication"
  type        = string
  sensitive   = true
}

variable "grafana_password" {
  description = "Grafana admin password"
  type        = string
  sensitive   = true
  default     = "changeme"
}

# Network security
variable "allowed_source_ips" {
  description = "List of CIDR blocks allowed to access the server (ports 22, 9000, 9090, 3000)"
  type        = list(string)
  default     = []
}

# Docker image versions
variable "prometheus_version" {
  description = "Prometheus Docker image version"
  type        = string
  default     = "v2.54.1"
}

variable "grafana_version" {
  description = "Grafana Docker image version"
  type        = string
  default     = "11.3.0"
}

variable "loki_version" {
  description = "Loki Docker image version"
  type        = string
  default     = "3.0.0"
}

variable "promtail_version" {
  description = "Promtail Docker image version"
  type        = string
  default     = "3.0.0"
}
