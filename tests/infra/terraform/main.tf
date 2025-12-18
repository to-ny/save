provider "digitalocean" {
  token = var.do_token
}

resource "digitalocean_container_registry" "save" {
  name                   = "save-registry"
  subscription_tier_slug = "basic"
}

resource "digitalocean_kubernetes_cluster" "save" {
  name    = var.cluster_name
  region  = var.region
  version = "1.32.10-do.1"

  registry_integration = true

  node_pool {
    name       = "worker-pool"
    size       = var.node_size
    node_count = var.node_count
  }

  depends_on = [digitalocean_container_registry.save]
}

provider "kubernetes" {
  host                   = digitalocean_kubernetes_cluster.save.endpoint
  token                  = digitalocean_kubernetes_cluster.save.kube_config[0].token
  cluster_ca_certificate = base64decode(digitalocean_kubernetes_cluster.save.kube_config[0].cluster_ca_certificate)
}

provider "helm" {
  kubernetes {
    host                   = digitalocean_kubernetes_cluster.save.endpoint
    token                  = digitalocean_kubernetes_cluster.save.kube_config[0].token
    cluster_ca_certificate = base64decode(digitalocean_kubernetes_cluster.save.kube_config[0].cluster_ca_certificate)
  }
}

# Write kubeconfig to a local file for kubectl commands
# Using local_sensitive_file to avoid leaking secrets in logs
resource "local_sensitive_file" "kubeconfig" {
  content         = digitalocean_kubernetes_cluster.save.kube_config[0].raw_config
  filename        = "${path.module}/.kubeconfig"
  file_permission = "0600"
}
