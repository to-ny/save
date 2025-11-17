#!/usr/bin/env bash

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

log_info "Checking load test infrastructure prerequisites..."
echo ""

ERRORS=0
WARNINGS=0

check_env() {
  if [[ -n "${!1:-}" ]]; then
    echo -e "${GREEN}OK${NC} $1 is set"
    return 0
  else
    echo -e "${YELLOW}WARN${NC} $1 is not set"
    ((WARNINGS++))
    return 1
  fi
}

check_file() {
  if [[ -f "$1" ]]; then
    echo -e "${GREEN}OK${NC} $1 exists"
    return 0
  else
    echo -e "${RED}FAIL${NC} $1 not found"
    ((ERRORS++))
    return 1
  fi
}

echo "==> Required tools"
check_command terraform || ((ERRORS++))
check_command docker || ((ERRORS++))
check_command ssh || ((ERRORS++))
check_command scp || ((ERRORS++))
check_command curl || ((ERRORS++))
echo ""

echo "==> Docker daemon"
if docker info >/dev/null 2>&1; then
  echo -e "${GREEN}OK${NC} Docker daemon is running"
else
  echo -e "${RED}FAIL${NC} Docker daemon is not running"
  echo "  Run: sudo systemctl start docker"
  ((ERRORS++))
fi
echo ""

echo "==> Optional tools"
if check_command jq false; then
  :
else
  echo "  Note: jq enables better cleanup script functionality"
  echo "  Install: apt-get install jq / brew install jq"
fi

if check_command aws false; then
  :
else
  echo "  Note: AWS CLI required for smoke tests"
  echo "  Install: https://aws.amazon.com/cli/"
fi
echo ""

echo "==> SSH configuration"
check_file "${HOME}/.ssh/id_rsa.pub"
echo ""

echo "==> Environment variables (required for deployment)"
check_env HCLOUD_TOKEN || echo "  Run: export HCLOUD_TOKEN=your_token"
echo ""

echo "==> Credentials (required for deployment)"
check_env SAVE_ACCESS_KEY || echo "  Run: export SAVE_ACCESS_KEY=your_access_key"
check_env SAVE_SECRET_KEY || echo "  Run: export SAVE_SECRET_KEY=your_secret_key"
echo ""

echo "==> Terraform initialization"
if [[ -d "${SCRIPT_DIR}/../terraform/.terraform" ]]; then
  echo -e "${GREEN}OK${NC} Terraform initialized"
else
  echo -e "${YELLOW}WARN${NC} Terraform not initialized"
  echo "  Run: cd tests/infra/terraform && terraform init"
  ((WARNINGS++))
fi
echo ""

if [[ $ERRORS -eq 0 && $WARNINGS -eq 0 ]]; then
  log_success "All checks passed! Ready to deploy."
  exit 0
elif [[ $ERRORS -eq 0 ]]; then
  log_warn "$WARNINGS warnings found. You can proceed but may need to configure some things."
  exit 0
else
  log_error "$ERRORS errors found. Please fix the issues above before deploying."
  exit 1
fi
