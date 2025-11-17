#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/../terraform"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

if [[ ! -f "${TF_DIR}/terraform.tfstate" ]]; then
  log_error "No Terraform state found. No server deployed."
  exit 1
fi

if [[ -z "${HCLOUD_TOKEN:-}" ]]; then
  log_error "HCLOUD_TOKEN environment variable not set"
  exit 1
fi

cd "${TF_DIR}"

SERVER_IP=$(terraform output -raw server_ip 2>/dev/null || echo "")
if [[ -z "$SERVER_IP" ]]; then
  log_error "Could not get server IP from Terraform state"
  exit 1
fi

log_info "Diagnosing server at: ${SERVER_IP}"
echo ""

echo "==> Testing SSH connection..."
if ssh -o ConnectTimeout=5 "root@${SERVER_IP}" "echo SSH OK" 2>/dev/null; then
  log_success "SSH connection working"
else
  log_error "SSH connection failed"
  exit 1
fi
echo ""

echo "==> Docker daemon status:"
ssh "root@${SERVER_IP}" "systemctl status docker --no-pager" | head -n 5
echo ""

echo "==> Docker containers:"
ssh "root@${SERVER_IP}" "cd /opt/save && docker-compose ps" || echo "docker-compose not found or failed"
echo ""

echo "==> Docker images:"
ssh "root@${SERVER_IP}" "docker images | grep -E 'save-api|prometheus|REPOSITORY'"
echo ""

echo "==> save-api container logs (last 30 lines):"
ssh "root@${SERVER_IP}" "cd /opt/save && docker-compose logs --tail=30 save-api" || echo "Could not get logs"
echo ""

echo "==> Testing health endpoint:"
ENDPOINT="http://${SERVER_IP}:9000/health"
if curl -sf "$ENDPOINT" 2>/dev/null; then
  log_success "Health check passed: $ENDPOINT"
else
  log_error "Health check failed: $ENDPOINT"
  echo ""
  echo "==> Testing port connectivity:"
  if nc -zv -w 5 "$SERVER_IP" 9000 2>&1 | grep -q succeeded; then
    log_warn "Port 9000 is reachable but health endpoint not responding"
  else
    log_error "Port 9000 is not reachable"
  fi
fi
echo ""

echo "==> Local firewall (iptables):"
ssh "root@${SERVER_IP}" "iptables -L INPUT -n | grep -E 'Chain|9000|9090|22' || echo 'No iptables rules found'"
echo ""

echo "==> Hetzner Cloud firewall status:"
if command -v jq &>/dev/null; then
  SERVER_NAME=$(terraform output -raw server_name 2>/dev/null || echo "")
  if [[ -n "$SERVER_NAME" ]]; then
    SERVER_ID=$(curl -s -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
      "https://api.hetzner.cloud/v1/servers?name=${SERVER_NAME}" | \
      jq -r '.servers[0].id // empty')

    if [[ -n "$SERVER_ID" ]]; then
      FIREWALLS=$(curl -s -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
        "https://api.hetzner.cloud/v1/servers/${SERVER_ID}" | \
        jq -r '.server.public_net.firewalls // []')

      if [[ "$FIREWALLS" == "[]" ]]; then
        log_error "No Hetzner Cloud firewall attached to server!"
        echo "  This is the problem - ports are blocked by default"
      else
        log_success "Hetzner Cloud firewall attached:"
        echo "$FIREWALLS" | jq -r '.[] | "  Firewall ID: \(.id), Status: \(.status)"'
      fi
    fi
  fi
else
  log_warn "Skipped (requires jq): install with apt-get install jq / brew install jq"
fi
echo ""

echo "==> Disk space:"
ssh "root@${SERVER_IP}" "df -h / /var/lib/docker"
echo ""

echo "==> Disk usage by directory:"
ssh "root@${SERVER_IP}" "du -sh /opt/save /var/lib/docker/volumes/* 2>/dev/null | sort -h" || echo "Could not get disk usage"
echo ""

echo "==> Docker system disk usage:"
ssh "root@${SERVER_IP}" "docker system df" || echo "Could not get Docker disk usage"
echo ""

echo "==> Memory usage:"
ssh "root@${SERVER_IP}" "free -h"
echo ""

echo "==> CPU load:"
ssh "root@${SERVER_IP}" "uptime"
echo ""

log_info "To connect and debug manually:"
echo "  ssh root@${SERVER_IP}"
echo "  cd /opt/save"
echo "  docker-compose logs -f save-api"
echo "  docker-compose restart save-api"
echo ""
