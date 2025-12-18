variable "do_token" {
  description = "DigitalOcean API token"
  type        = string
  sensitive   = true
}

variable "cluster_name" {
  description = "Name of the Kubernetes cluster"
  type        = string
  default     = "save-loadtest"
}

variable "region" {
  description = "Region to deploy the cluster"
  type        = string
  default     = "nyc1"
}

variable "node_size" {
  description = "Size of the worker nodes"
  type        = string
  default     = "s-4vcpu-8gb"
}

variable "node_count" {
  description = "Number of worker nodes"
  type        = number
  default     = 1
}

variable "save_profile" {
  description = "Save deployment profile (smoke, medium, large)"
  type        = string
  default     = "medium"
}

variable "test_name" {
  description = "Load test to run"
  type        = string
  default     = "test_quick_smoke"
}

