# Build and push images to DO registry
resource "null_resource" "build_push_images" {
  triggers = {
    cluster_id = digitalocean_kubernetes_cluster.save.id
  }

  provisioner "local-exec" {
    command = <<-EOT
      set -e
      # TODO: Remove DOCKER_API_VERSION override once docker client is upgraded (required for WSL with old docker client)
      export DOCKER_API_VERSION=1.44
      cd ${path.module}/../../..

      # Login to DO registry
      doctl registry login

      # Build and push Save image
      docker build -t registry.digitalocean.com/${digitalocean_container_registry.save.name}/save:latest .
      docker push registry.digitalocean.com/${digitalocean_container_registry.save.name}/save:latest

      # Build and push loadtest image
      docker build -f tests/infra/Dockerfile.loadtest -t registry.digitalocean.com/${digitalocean_container_registry.save.name}/save-loadtest:latest .
      docker push registry.digitalocean.com/${digitalocean_container_registry.save.name}/save-loadtest:latest
    EOT
  }

  depends_on = [
    digitalocean_container_registry.save,
    digitalocean_kubernetes_cluster.save
  ]
}

# Create registry pull secret in save-test namespace
resource "null_resource" "registry_secret" {
  triggers = {
    cluster_id = digitalocean_kubernetes_cluster.save.id
  }

  provisioner "local-exec" {
    command = <<-EOT
      doctl registry kubernetes-manifest | sed 's/namespace: kube-system/namespace: save-test/' | kubectl --kubeconfig="${local_sensitive_file.kubeconfig.filename}" apply -f -
    EOT
  }

  depends_on = [null_resource.build_push_images, local_sensitive_file.kubeconfig]
}

# Deploy Save cluster
resource "helm_release" "save" {
  name             = "save"
  chart            = "${path.module}/../../../charts/save"
  namespace        = "save-test"
  create_namespace = true
  wait             = true
  timeout          = 600

  values = [
    file("${path.module}/../helm/values-${var.save_profile}.yaml")
  ]

  set {
    name  = "image.repository"
    value = "registry.digitalocean.com/${digitalocean_container_registry.save.name}/save"
  }

  set {
    name  = "image.tag"
    value = "latest"
  }

  set {
    name  = "image.pullPolicy"
    value = "Always"
  }

  set {
    name  = "imagePullSecrets[0].name"
    value = "registry-${digitalocean_container_registry.save.name}"
  }

  set {
    name  = "fullnameOverride"
    value = "save"
  }

  depends_on = [null_resource.registry_secret]
}

# Run load test as Kubernetes Job
resource "kubernetes_job" "loadtest" {
  metadata {
    name      = "save-loadtest"
    namespace = "save-test"
  }

  spec {
    backoff_limit = 0

    template {
      metadata {
        labels = {
          app = "save-loadtest"
        }
      }

      spec {
        restart_policy = "Never"

        image_pull_secrets {
          name = "registry-${digitalocean_container_registry.save.name}"
        }

        container {
          name  = "loadtest"
          image = "registry.digitalocean.com/${digitalocean_container_registry.save.name}/save-loadtest:latest"

          env {
            name  = "SAVE_ENDPOINT"
            value = "http://save:9000"
          }

          env {
            name = "SAVE_ACCESS_KEY"
            value_from {
              secret_key_ref {
                name = "save-credentials"
                key  = "access-key"
              }
            }
          }

          env {
            name = "SAVE_SECRET_KEY"
            value_from {
              secret_key_ref {
                name = "save-credentials"
                key  = "secret-key"
              }
            }
          }

          env {
            name  = "SAVE_BUCKET"
            value = "loadtest"
          }

          env {
            name  = "TEST_FILTER"
            value = var.test_name
          }

          args = ["--test-threads=1", "--nocapture", var.test_name]

          resources {
            requests = {
              cpu    = "500m"
              memory = "512Mi"
            }
            limits = {
              cpu    = "2"
              memory = "2Gi"
            }
          }
        }
      }
    }
  }

  wait_for_completion = true
  timeouts {
    create = "30m"
  }

  depends_on = [helm_release.save]
}
