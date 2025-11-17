#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/../terraform"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

setup_error_handling

if [[ ! -f "${TF_DIR}/terraform.tfstate" ]]; then
  log_error "No Terraform state found. Deploy a server first with:"
  echo "  ./tests/infra/deploy.sh [smoke|medium|large]"
  exit 1
fi

cd "${TF_DIR}"

SERVER_IP=$(terraform output -raw server_ip 2>/dev/null || echo "")
SAVE_ENDPOINT=$(terraform output -raw save_endpoint 2>/dev/null || echo "")
PROFILE=$(terraform output -json | grep -o '"profile"[[:space:]]*:[[:space:]]*"[^"]*"' | cut -d'"' -f4 || echo "medium")

if [[ -z "${SERVER_IP}" ]]; then
  log_error "Could not get server IP from Terraform state"
  exit 1
fi

log_info "Updating save-api on server: ${SERVER_IP} (profile: ${PROFILE})"

if [[ -t 0 ]]; then
  SAVE_ACCESS_KEY=$(get_credential "SAVE_ACCESS_KEY" "Enter S3 access key" false)
  SAVE_SECRET_KEY=$(get_credential "SAVE_SECRET_KEY" "Enter S3 secret key" true)
else
  if [[ -z "${SAVE_ACCESS_KEY:-}" ]] || [[ -z "${SAVE_SECRET_KEY:-}" ]]; then
    log_error "SAVE_ACCESS_KEY and SAVE_SECRET_KEY must be set in non-interactive mode"
    exit 1
  fi
fi

CONFIG_FILE="/tmp/save-config-$(date +%s).toml"
register_temp_file "$CONFIG_FILE"
generate_save_config "$PROFILE" "$SAVE_ACCESS_KEY" "$SAVE_SECRET_KEY" "$CONFIG_FILE"

log_info "Building Docker image locally..."
cd "${SCRIPT_DIR}/../../.."
docker build -t save-api:latest -f Dockerfile .

log_info "Saving Docker image to tarball..."
IMAGE_TARBALL="/tmp/save-api-$(date +%s).tar.gz"
register_temp_file "$IMAGE_TARBALL"
docker save save-api:latest | gzip > "$IMAGE_TARBALL"

log_info "Creating backup of current image on server..."
ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" << 'ENDSSH'
cd /opt/save
if docker images | grep -q "save-api.*latest"; then
  docker tag save-api:latest save-api:backup || true
fi
ENDSSH

log_info "Uploading Docker image and config to server..."
scp -o StrictHostKeyChecking=no "$IMAGE_TARBALL" "root@${SERVER_IP}:/opt/save/save-api.tar.gz"
scp -o StrictHostKeyChecking=no "$CONFIG_FILE" "root@${SERVER_IP}:/opt/save/save.toml"

log_info "Loading image and restarting services..."
ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" << 'ENDSSH'
cd /opt/save
echo "Stopping containers..."
docker-compose down
echo "Loading new save-api image..."
docker load < save-api.tar.gz
rm save-api.tar.gz
echo "Starting containers..."
docker-compose up -d
echo "Waiting for containers to start..."
sleep 5
echo "Checking container status..."
docker-compose ps
ENDSSH

log_info "Waiting for save-api to be ready..."
HEALTH_OK=false
for i in {1..30}; do
  HTTP_CODE=$(curl -sf -o /dev/null -w "%{http_code}" "${SAVE_ENDPOINT}/health" 2>/dev/null || echo "000")

  if [[ "$HTTP_CODE" == "200" ]]; then
    log_success "save-api is healthy and reachable!"
    HEALTH_OK=true
    break
  fi

  if [[ $i -eq 1 ]] || [[ $((i % 6)) -eq 0 ]]; then
    ELAPSED=$((i * 2))
    echo "Health check attempt ${i}: HTTP ${HTTP_CODE} (${ELAPSED}s elapsed)"
  fi

  sleep 2
done

if [[ "$HEALTH_OK" == "false" ]]; then
  echo ""
  log_error "Health check failed after update"
  log_warn "Attempting rollback to previous version..."

  ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" << 'ENDSSH'
cd /opt/save
if docker images | grep -q "save-api.*backup"; then
  echo "Rolling back to backup image..."
  docker-compose down
  docker tag save-api:backup save-api:latest
  docker-compose up -d
  sleep 5
  docker-compose ps
  echo "Rollback complete. Previous image restored."
else
  echo "No backup image found. Manual intervention required."
fi
ENDSSH

  log_info "Checking logs after rollback attempt..."
  ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" "cd /opt/save && docker-compose logs --tail=50 save-api"
  exit 1
fi

ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" << 'ENDSSH'
if docker images | grep -q "save-api.*backup"; then
  docker rmi save-api:backup 2>/dev/null || true
fi
ENDSSH

echo ""
log_success "Update complete!"
echo "    Endpoint: ${SAVE_ENDPOINT}"
echo ""
