#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/../terraform"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

if [[ -z "${HCLOUD_TOKEN:-}" ]]; then
  log_error "HCLOUD_TOKEN environment variable not set"
  exit 1
fi

if [[ ! -f "${TF_DIR}/terraform.tfstate" ]]; then
  log_info "No Terraform state found. Nothing to tear down."
  exit 0
fi

log_info "Destroying load test infrastructure..."
cd "${TF_DIR}"

# Note: We pass placeholder values for credentials since they're not needed for destroy
terraform destroy \
  -var "hcloud_token=${HCLOUD_TOKEN}" \
  -var "ssh_public_key=placeholder" \
  -var "save_access_key=placeholder" \
  -var "save_secret_key=placeholder" \
  -auto-approve

log_success "Infrastructure destroyed successfully"
