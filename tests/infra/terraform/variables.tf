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

variable "storage_type" {
  description = "Storage backend: empty for local disk, 'volume' to create and attach Hetzner volume"
  type        = string
  default     = ""

  validation {
    condition     = var.storage_type == "" || var.storage_type == "volume"
    error_message = "storage_type must be empty (local disk) or 'volume' (Hetzner volume)"
  }
}

variable "volume_size_gb" {
  description = "Size of Hetzner volume in GB (minimum 10, only used when storage_type='volume')"
  type        = number
  default     = 10

  validation {
    condition     = var.volume_size_gb >= 10
    error_message = "volume_size_gb must be at least 10 GB"
  }
}

variable "worker_threads" {
  description = "Number of worker threads for save-api"
  type        = number
  default     = 4
}

variable "write_buffer_size_mb" {
  description = "RocksDB write buffer size in MB"
  type        = number
  default     = 256
}

variable "block_cache_size_mb" {
  description = "RocksDB block cache size in MB"
  type        = number
  default     = 512
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
