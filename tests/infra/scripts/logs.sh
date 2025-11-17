#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/../terraform"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

if [[ ! -f "${TF_DIR}/terraform.tfstate" ]]; then
  log_error "No Terraform state found. Deploy a server first with:"
  echo "  ./tests/infra/deploy.sh [smoke|medium|large]"
  exit 1
fi

cd "${TF_DIR}"

SERVER_IP=$(terraform output -raw server_ip 2>/dev/null || echo "")

if [[ -z "${SERVER_IP}" ]]; then
  log_error "Could not get server IP from Terraform state"
  exit 1
fi

FOLLOW=""
TAIL_LINES="50"
SERVICE="save-api"

while [[ $# -gt 0 ]]; do
  case $1 in
    -f|--follow)
      FOLLOW="-f"
      shift
      ;;
    -n|--tail)
      TAIL_LINES="$2"
      shift 2
      ;;
    --all)
      SERVICE=""
      shift
      ;;
    --prometheus)
      SERVICE="prometheus"
      shift
      ;;
    --grafana)
      SERVICE="grafana"
      shift
      ;;
    --loki)
      SERVICE="loki"
      shift
      ;;
    *)
      echo "Unknown option: $1"
      echo "Usage: $0 [-f|--follow] [-n|--tail LINES] [--all|--prometheus|--grafana|--loki]"
      echo "  -f, --follow      Follow log output"
      echo "  -n, --tail LINES  Number of lines to show (default: 50)"
      echo "  --all             Show logs for all services"
      echo "  --prometheus      Show Prometheus logs"
      echo "  --grafana         Show Grafana logs"
      echo "  --loki            Show Loki logs"
      exit 1
      ;;
  esac
done

log_info "Fetching logs from ${SERVER_IP}..."
echo ""

if [[ -n "$SERVICE" ]]; then
  ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" \
    "cd /opt/save && docker-compose logs --tail=${TAIL_LINES} ${FOLLOW} ${SERVICE}"
else
  ssh -o StrictHostKeyChecking=no "root@${SERVER_IP}" \
    "cd /opt/save && docker-compose logs --tail=${TAIL_LINES} ${FOLLOW}"
fi
