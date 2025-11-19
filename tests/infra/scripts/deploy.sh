#!/usr/bin/env bash

PROFILE="${1:-medium}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/../terraform"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

setup_error_handling

if [[ ! -f "${TF_DIR}/profiles/${PROFILE}.tfvars" ]]; then
  log_error "Profile '${PROFILE}' not found"
  echo "Available profiles: smoke, medium, large"
  exit 1
fi

log_info "Deploying save-api with profile: ${PROFILE}"

if [[ -z "${HCLOUD_TOKEN:-}" ]]; then
  log_error "HCLOUD_TOKEN environment variable not set"
  echo "Export your Hetzner Cloud API token:"
  echo "  export HCLOUD_TOKEN=your_token_here"
  exit 1
fi

if [[ -z "${SSH_PUBLIC_KEY:-}" ]]; then
  SSH_PUBLIC_KEY_FILE="${HOME}/.ssh/id_rsa.pub"
  if [[ ! -f "${SSH_PUBLIC_KEY_FILE}" ]]; then
    log_error "SSH public key not found at ${SSH_PUBLIC_KEY_FILE}"
    echo "Generate one with: ssh-keygen -t rsa -b 4096"
    echo "Or set SSH_PUBLIC_KEY environment variable"
    exit 1
  fi
  SSH_PUBLIC_KEY="$(cat "${SSH_PUBLIC_KEY_FILE}")"
fi

if [[ -t 0 ]]; then
  SAVE_ACCESS_KEY=$(get_credential "SAVE_ACCESS_KEY" "Enter S3 access key" false)
  SAVE_SECRET_KEY=$(get_credential "SAVE_SECRET_KEY" "Enter S3 secret key" true)
  GRAFANA_PASSWORD=$(get_credential "GRAFANA_PASSWORD" "Enter Grafana admin password (default: changeme)" true)

  if [[ -z "$GRAFANA_PASSWORD" ]]; then
    GRAFANA_PASSWORD="changeme"
    log_warn "Using default Grafana password 'changeme' - not recommended for production!"
  fi
else
  if [[ -z "${SAVE_ACCESS_KEY:-}" ]] || [[ -z "${SAVE_SECRET_KEY:-}" ]]; then
    log_error "SAVE_ACCESS_KEY and SAVE_SECRET_KEY must be set in non-interactive mode"
    exit 1
  fi
  GRAFANA_PASSWORD="${GRAFANA_PASSWORD:-changeme}"
fi

log_info "Detecting local public IP for firewall..."
if LOCAL_IP=$(get_local_public_ip); then
  log_success "Detected local IP: $LOCAL_IP"
  ALLOWED_IPS="${LOCAL_IP}/32"

  if [[ -n "${ALLOWED_SOURCE_IPS:-}" ]]; then
    ALLOWED_IPS="${ALLOWED_IPS},${ALLOWED_SOURCE_IPS}"
    log_info "Additional allowed IPs: ${ALLOWED_SOURCE_IPS}"
  fi
else
  log_warn "Could not detect local IP, firewall will allow all IPs (0.0.0.0/0)"
  ALLOWED_IPS="0.0.0.0/0,::/0"
fi

TF_ALLOWED_IPS="[\"$(echo "$ALLOWED_IPS" | sed 's/,/","/g')\"]"

log_info "Running Terraform with profile: ${PROFILE}"
cd "${TF_DIR}"

terraform init -upgrade

terraform apply \
  -var "hcloud_token=${HCLOUD_TOKEN}" \
  -var "ssh_public_key=${SSH_PUBLIC_KEY}" \
  -var "save_access_key=${SAVE_ACCESS_KEY}" \
  -var "save_secret_key=${SAVE_SECRET_KEY}" \
  -var "grafana_password=${GRAFANA_PASSWORD}" \
  -var "allowed_source_ips=${TF_ALLOWED_IPS}" \
  -var-file="profiles/${PROFILE}.tfvars" \
  -auto-approve

SERVER_IP=$(terraform output -raw server_ip)
SAVE_ENDPOINT=$(terraform output -raw save_endpoint)
STORAGE_BACKEND=$(terraform output -raw storage_backend)

log_success "Infrastructure provisioned at: ${SERVER_IP}"

if [[ -f "${HOME}/.ssh/known_hosts" ]]; then
  ssh-keygen -R "${SERVER_IP}" &>/dev/null || true
fi

log_info "Building Docker image..."
cd "${SCRIPT_DIR}/../../.."
docker build -t save-api:latest -f Dockerfile .

log_info "Saving Docker image to tarball..."
IMAGE_TARBALL="/tmp/save-api-$(date +%s).tar.gz"
register_temp_file "$IMAGE_TARBALL"
docker save save-api:latest | gzip > "$IMAGE_TARBALL"

log_info "Waiting for server to be ready..."
"${SCRIPT_DIR}/wait-for-server.sh" "${SERVER_IP}"

log_info "Uploading Docker image to server..."
scp "$IMAGE_TARBALL" "root@${SERVER_IP}:/opt/save/save-api.tar.gz"

log_info "Uploading deployment script..."
scp "${SCRIPT_DIR}/deploy-image.sh" "root@${SERVER_IP}:/tmp/deploy-image.sh"

log_info "Deploying image..."
if ssh "root@${SERVER_IP}" "chmod +x /tmp/deploy-image.sh && /tmp/deploy-image.sh"; then
  log_success "Deployment successful!"
else
  log_error "Deployment failed, check logs with: ssh root@${SERVER_IP} 'cd /opt/save && docker-compose logs save-api'"
  exit 1
fi

log_info "Waiting for save-api to be ready..."
HEALTH_OK=false
for i in {1..60}; do
  HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" "${SAVE_ENDPOINT}/health" 2>/dev/null || echo "000")

  if [[ "$HTTP_CODE" == "200" ]]; then
    log_success "save-api is healthy and reachable!"
    HEALTH_OK=true
    break
  fi

  if [[ $i -eq 1 ]] || [[ $((i % 12)) -eq 0 ]]; then
    ELAPSED=$((i * 5))
    echo "Health check attempt ${i}: HTTP ${HTTP_CODE} (${ELAPSED}s elapsed)"
  fi

  sleep 5
done

if [[ "$HEALTH_OK" == "false" ]]; then
  echo ""
  log_error "External health check failed after 5 minutes"
  echo ""
  log_info "Checking server-side health..."
  ssh "root@${SERVER_IP}" "curl -v http://localhost:9000/health" 2>&1 | head -10
  echo ""
  log_info "Container logs:"
  ssh "root@${SERVER_IP}" "cd /opt/save && docker-compose logs --tail=30 save-api"
  exit 1
fi

if run_smoke_tests "$SAVE_ENDPOINT" "$SAVE_ACCESS_KEY" "$SAVE_SECRET_KEY"; then
  log_success "Smoke tests passed!"
else
  log_warn "Smoke tests failed - deployment completed but functionality may be impaired"
fi

case "$PROFILE" in
  smoke)
    SERVER_TYPE="cpx11"
    VCPU=2
    RAM_GB=2
    ;;
  medium)
    SERVER_TYPE="cpx31"
    VCPU=4
    RAM_GB=8
    ;;
  large)
    SERVER_TYPE="cpx41"
    VCPU=8
    RAM_GB=16
    ;;
  *)
    SERVER_TYPE="unknown"
    VCPU=0
    RAM_GB=0
    ;;
esac

echo ""
echo "==> Deployment Complete"
echo "Profile: ${PROFILE} (${VCPU} vCPU, ${RAM_GB}GB RAM)"
echo "Server IP: ${SERVER_IP}"
echo "Endpoint: ${SAVE_ENDPOINT}"
echo "Prometheus: http://${SERVER_IP}:9090"
echo "Grafana: http://${SERVER_IP}:3000"
echo ""
echo "Next steps:"
echo "  source ${SCRIPT_DIR}/../.env.loadtest"
echo "  cargo test -p save-loadtest --features load_tests"
echo ""
echo "Teardown: make teardown"
echo ""

cat > "${SCRIPT_DIR}/../.env.loadtest" <<EOF

export SAVE_ENDPOINT=${SAVE_ENDPOINT}
export SAVE_ACCESS_KEY=${SAVE_ACCESS_KEY}
export SAVE_SECRET_KEY=${SAVE_SECRET_KEY}
export SAVE_SERVER_TYPE=${SERVER_TYPE}
export SAVE_SERVER_VCPU=${VCPU}
export SAVE_SERVER_RAM_GB=${RAM_GB}
export SAVE_SERVER_PROFILE=${PROFILE}
export SAVE_SERVER_OS=Linux
export SAVE_SERVER_OS_VERSION="Ubuntu 24.04"
EOF

if [[ -n "${STORAGE_BACKEND}" ]]; then
  echo "export SAVE_STORAGE_TYPE=${STORAGE_BACKEND}" >> "${SCRIPT_DIR}/../.env.loadtest"
fi

log_success "Server metadata saved to: ${SCRIPT_DIR}/../.env.loadtest"
echo ""
