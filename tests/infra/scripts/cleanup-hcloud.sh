#!/usr/bin/env bash
set -euo pipefail

DELETE_MODE=false
if [[ "${1:-}" == "--delete" ]]; then
  DELETE_MODE=true
fi

if [[ -z "${HCLOUD_TOKEN:-}" ]]; then
  echo "Error: HCLOUD_TOKEN environment variable not set"
  exit 1
fi

if [[ "$DELETE_MODE" == "true" ]] && ! command -v jq &>/dev/null; then
  echo "Error: jq is required for deletion mode"
  echo "Install with: apt-get install jq / brew install jq"
  exit 1
fi

USE_JQ=false
if command -v jq &>/dev/null; then
  USE_JQ=true
fi

echo "==> Finding orphaned save-loadtest resources..."
echo ""

SERVERS=$(curl -s -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
  https://api.hetzner.cloud/v1/servers)

FIREWALLS=$(curl -s -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
  https://api.hetzner.cloud/v1/firewalls)

SSH_KEYS=$(curl -s -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
  https://api.hetzner.cloud/v1/ssh_keys)

echo "==> Servers with 'save-loadtest' prefix:"
if [[ "$USE_JQ" == "true" ]]; then
  SERVER_IDS=$(echo "$SERVERS" | jq -r '.servers[] | select(.name | startswith("save-loadtest")) | .id')
  echo "$SERVERS" | jq -r '.servers[] | select(.name | startswith("save-loadtest")) | "\(.id)\t\(.name)\t\(.status)"' || echo "None found"
  SERVER_COUNT=$(echo "$SERVER_IDS" | grep -c . || echo 0)
else
  echo "$SERVERS" | grep -o '"id":[0-9]*,"name":"save-loadtest[^"]*"' || echo "None found"
  SERVER_COUNT=0
fi

echo ""
echo "==> Firewalls with 'save-loadtest' prefix:"
if [[ "$USE_JQ" == "true" ]]; then
  FIREWALL_IDS=$(echo "$FIREWALLS" | jq -r '.firewalls[] | select(.name | startswith("save-loadtest")) | .id')
  echo "$FIREWALLS" | jq -r '.firewalls[] | select(.name | startswith("save-loadtest")) | "\(.id)\t\(.name)"' || echo "None found"
  FIREWALL_COUNT=$(echo "$FIREWALL_IDS" | grep -c . || echo 0)
else
  echo "$FIREWALLS" | grep -o '"id":[0-9]*,"name":"save-loadtest[^"]*"' || echo "None found"
  FIREWALL_COUNT=0
fi

echo ""
echo "==> SSH Keys with 'save-loadtest' prefix:"
if [[ "$USE_JQ" == "true" ]]; then
  SSH_KEY_IDS=$(echo "$SSH_KEYS" | jq -r '.ssh_keys[] | select(.name | startswith("save-loadtest")) | .id')
  echo "$SSH_KEYS" | jq -r '.ssh_keys[] | select(.name | startswith("save-loadtest")) | "\(.id)\t\(.name)"' || echo "None found"
  SSH_KEY_COUNT=$(echo "$SSH_KEY_IDS" | grep -c . || echo 0)
else
  echo "$SSH_KEYS" | grep -o '"id":[0-9]*,"name":"save-loadtest[^"]*"' || echo "None found"
  SSH_KEY_COUNT=0
fi

echo ""

if [[ "$DELETE_MODE" == "false" ]]; then
  TOTAL=$((SERVER_COUNT + FIREWALL_COUNT + SSH_KEY_COUNT))
  if [[ $TOTAL -gt 0 ]]; then
    echo "Found $TOTAL orphaned resources."
    echo "To delete them, run: $0 --delete"
  else
    echo "No orphaned resources found."
  fi
  exit 0
fi

TOTAL=$((SERVER_COUNT + FIREWALL_COUNT + SSH_KEY_COUNT))
if [[ $TOTAL -eq 0 ]]; then
  echo "No orphaned resources to delete."
  exit 0
fi

echo "WARNING: About to delete $TOTAL resources:"
echo "  - $SERVER_COUNT servers"
echo "  - $FIREWALL_COUNT firewalls"
echo "  - $SSH_KEY_COUNT SSH keys"
echo ""
read -p "Are you sure? (yes/no): " -r CONFIRM

if [[ "$CONFIRM" != "yes" ]]; then
  echo "Aborted."
  exit 0
fi

echo ""
echo "==> Deleting servers..."
for id in $SERVER_IDS; do
  if [[ -n "$id" ]]; then
    echo "Deleting server ID: $id"
    curl -s -X DELETE \
      -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
      "https://api.hetzner.cloud/v1/servers/$id" > /dev/null
    echo "  Deleted"
  fi
done

echo ""
echo "==> Waiting for servers to be fully deleted..."
sleep 5

echo ""
echo "==> Deleting firewalls..."
for id in $FIREWALL_IDS; do
  if [[ -n "$id" ]]; then
    echo "Deleting firewall ID: $id"
    curl -s -X DELETE \
      -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
      "https://api.hetzner.cloud/v1/firewalls/$id" > /dev/null
    echo "  Deleted"
  fi
done

echo ""
echo "==> Deleting SSH keys..."
for id in $SSH_KEY_IDS; do
  if [[ -n "$id" ]]; then
    echo "Deleting SSH key ID: $id"
    curl -s -X DELETE \
      -H "Authorization: Bearer ${HCLOUD_TOKEN}" \
      "https://api.hetzner.cloud/v1/ssh_keys/$id" > /dev/null
    echo "  Deleted"
  fi
done

echo ""
echo "==> All orphaned save-loadtest resources deleted successfully!"
