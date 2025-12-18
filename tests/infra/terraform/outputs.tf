output "cluster_name" {
  description = "Kubernetes cluster name"
  value       = digitalocean_kubernetes_cluster.save.name
}

output "registry_endpoint" {
  description = "Container registry endpoint"
  value       = digitalocean_container_registry.save.endpoint
}

output "cluster_endpoint" {
  description = "Kubernetes API endpoint"
  value       = digitalocean_kubernetes_cluster.save.endpoint
  sensitive   = true
}

output "kubeconfig" {
  description = "Raw kubeconfig for kubectl access"
  value       = digitalocean_kubernetes_cluster.save.kube_config[0].raw_config
  sensitive   = true
}

output "test_name" {
  description = "Load test that was executed"
  value       = var.test_name
}

output "save_profile" {
  description = "Save deployment profile used"
  value       = var.save_profile
}
