terraform {
  required_version = ">= 1.0"
  required_providers {
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.45"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.6"
    }
  }
}

provider "hcloud" {
  token = var.hcloud_token
}

resource "random_id" "suffix" {
  byte_length = 4
}

# Common tags for all resources
locals {
  common_tags = {
    environment = "loadtest"
    managed-by  = "terraform"
    project     = "save"
    profile     = var.profile
  }
}

resource "hcloud_ssh_key" "loadtest" {
  name       = "save-loadtest-${var.profile}-${random_id.suffix.hex}"
  public_key = var.ssh_public_key
  labels     = local.common_tags

  lifecycle {
    create_before_destroy = true
  }
}

resource "hcloud_server" "save" {
  name         = "save-loadtest-${var.profile}-${random_id.suffix.hex}"
  server_type  = var.server_type
  location     = var.location
  image        = "ubuntu-24.04"
  ssh_keys     = [hcloud_ssh_key.loadtest.id]
  firewall_ids = [hcloud_firewall.save.id]

  user_data = templatefile("${path.module}/cloud-init.yml", {
    use_volume = var.storage_type == "volume"
    # Render config files with variable substitution
    docker_compose_yml = indent(6, templatefile("${path.module}/files/docker-compose.yml.tpl", {
      prometheus_version = var.prometheus_version
      grafana_version    = var.grafana_version
      loki_version       = var.loki_version
      promtail_version   = var.promtail_version
      grafana_password   = var.grafana_password
    }))
    save_toml = indent(6, templatefile("${path.module}/files/save.toml.tpl", {
      save_access_key      = var.save_access_key
      save_secret_key      = var.save_secret_key
      worker_threads       = var.worker_threads
      write_buffer_size_mb = var.write_buffer_size_mb
      block_cache_size_mb  = var.block_cache_size_mb
    }))
    # Static config files
    prometheus_yml          = indent(6, file("${path.module}/files/prometheus.yml"))
    loki_config_yml         = indent(6, file("${path.module}/files/loki-config.yml"))
    promtail_config_yml     = indent(6, file("${path.module}/files/promtail-config.yml"))
    grafana_datasources_yml = indent(6, file("${path.module}/files/grafana-datasources.yml"))
    readme_txt              = indent(6, file("${path.module}/files/README.txt"))
  })

  labels = merge(local.common_tags, {
    purpose = "save-loadtest"
  })

  public_net {
    ipv4_enabled = true
    ipv6_enabled = false
  }
}

resource "hcloud_volume" "data" {
  count    = var.storage_type == "volume" ? 1 : 0
  name     = "save-data-${var.profile}-${random_id.suffix.hex}"
  size     = var.volume_size_gb
  location = var.location
  format   = "ext4"
  labels   = local.common_tags
}

resource "hcloud_volume_attachment" "data" {
  count     = var.storage_type == "volume" ? 1 : 0
  volume_id = hcloud_volume.data[0].id
  server_id = hcloud_server.save.id
  automount = false
}

resource "hcloud_firewall" "save" {
  name   = "save-loadtest-${var.profile}-${random_id.suffix.hex}"
  labels = local.common_tags

  # SSH access
  rule {
    direction  = "in"
    protocol   = "tcp"
    port       = "22"
    source_ips = length(var.allowed_source_ips) > 0 ? var.allowed_source_ips : ["0.0.0.0/0", "::/0"]
  }

  # Save API
  rule {
    direction  = "in"
    protocol   = "tcp"
    port       = "9000"
    source_ips = length(var.allowed_source_ips) > 0 ? var.allowed_source_ips : ["0.0.0.0/0", "::/0"]
  }

  # Prometheus
  rule {
    direction  = "in"
    protocol   = "tcp"
    port       = "9090"
    source_ips = length(var.allowed_source_ips) > 0 ? var.allowed_source_ips : ["0.0.0.0/0", "::/0"]
  }

  # Grafana
  rule {
    direction  = "in"
    protocol   = "tcp"
    port       = "3000"
    source_ips = length(var.allowed_source_ips) > 0 ? var.allowed_source_ips : ["0.0.0.0/0", "::/0"]
  }
}

