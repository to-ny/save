#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TF_DIR="${SCRIPT_DIR}/../terraform"

# shellcheck source=common.sh
source "${SCRIPT_DIR}/common.sh"

setup_error_handling

log_info "Phase 1: SSH-based local testing (avoiding WAN bottleneck)"

# Get server IP from Terraform
cd "${TF_DIR}"
if ! SERVER_IP=$(terraform output -raw server_ip 2>/dev/null); then
    log_error "No infrastructure deployed. Run 'make deploy-medium' first"
    exit 1
fi

log_info "Server IP: ${SERVER_IP}"

# Load credentials
if [[ ! -f "${SCRIPT_DIR}/../.env.loadtest" ]]; then
    log_error "Environment file not found. Run 'make deploy-medium' first"
    exit 1
fi

# shellcheck source=../.env.loadtest
source "${SCRIPT_DIR}/../.env.loadtest"

log_info "Preparing server for local testing..."
ssh "root@${SERVER_IP}" "mkdir -p /opt/loadtest/workspace /opt/loadtest/results"

log_info "Syncing project code to server..."
cd "${SCRIPT_DIR}/../../.."

# Create proper directory structure on server
ssh "root@${SERVER_IP}" "rm -rf /opt/loadtest/workspace && mkdir -p /opt/loadtest/workspace"

# Sync entire workspace (needed for Cargo.toml workspace references)
rsync -az --exclude 'target' --exclude '.git' --exclude 'docs' --exclude 'prompt_context' \
  . "root@${SERVER_IP}:/opt/loadtest/workspace/"

log_info "Ensuring build tools are installed on server..."
ssh "root@${SERVER_IP}" bash << 'BUILD_TOOLS'
if ! command -v cc &> /dev/null || ! dpkg -l | grep -q libssl-dev; then
    echo "Installing build dependencies..."
    apt-get update -qq
    apt-get install -y -qq build-essential pkg-config libssl-dev
    echo "Build tools installed"
else
    echo "Build tools already installed"
fi
BUILD_TOOLS

log_info "Ensuring Rust is installed on server..."
ssh "root@${SERVER_IP}" bash << 'RUST_INSTALL'
if ! command -v cargo &> /dev/null; then
    echo "Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    echo "Rust installed successfully"
else
    echo "Rust already installed"
fi
RUST_INSTALL

log_info "Running tests on server (localhost endpoint - no WAN)..."
log_info "This eliminates network bottleneck and shows true storage/app performance"

# Run test on server targeting localhost
ssh "root@${SERVER_IP}" bash << EOF
set -e

# Add cargo to PATH
export PATH="\$HOME/.cargo/bin:\$PATH"

export SAVE_ENDPOINT="http://localhost:9000"
export SAVE_ACCESS_KEY="${SAVE_ACCESS_KEY}"
export SAVE_SECRET_KEY="${SAVE_SECRET_KEY}"
export SAVE_SERVER_TYPE="${SAVE_SERVER_TYPE}"
export SAVE_SERVER_VCPU="${SAVE_SERVER_VCPU}"
export SAVE_SERVER_RAM_GB="${SAVE_SERVER_RAM_GB}"
export SAVE_SERVER_PROFILE="${SAVE_SERVER_PROFILE}"
export SAVE_SERVER_OS="${SAVE_SERVER_OS}"
export SAVE_SERVER_OS_VERSION="${SAVE_SERVER_OS_VERSION}"
export SAVE_STORAGE_TYPE="${SAVE_STORAGE_TYPE:-local-nvme}"

cd /opt/loadtest/workspace

echo "Building and running mixed workload test (this will take a few minutes)..."
cargo test --release -p save-loadtest --features load_tests test_mixed_workload -- --nocapture 2>&1 | tail -100

echo ""
echo "Test completed. Checking for results..."
find tests/loadtest/loadtest-results -name "mixed-*.md" -type f -mmin -10 2>/dev/null | head -5
EOF

log_info "Downloading results..."
RESULTS_DIR="${SCRIPT_DIR}/../../loadtest/loadtest-results"
mkdir -p "${RESULTS_DIR}"

# Download latest result
LATEST_RESULT=$(ssh "root@${SERVER_IP}" "find /opt/loadtest/workspace/tests/loadtest/loadtest-results -name 'mixed-*.md' -type f -mmin -10 2>/dev/null | sort -r | head -1" || echo "")

if [[ -n "${LATEST_RESULT}" ]]; then
    RESULT_FILE=$(basename "${LATEST_RESULT}")
    # Rename to indicate local testing
    LOCAL_RESULT="${RESULT_FILE%.md}-local.md"
    scp "root@${SERVER_IP}:${LATEST_RESULT}" "${RESULTS_DIR}/${LOCAL_RESULT}"
    log_success "Results downloaded to: ${RESULTS_DIR}/${LOCAL_RESULT}"

    echo ""
    log_info "Quick Summary:"
    echo ""
    grep -E "Storage:|Total Requests|Requests/sec" "${RESULTS_DIR}/${LOCAL_RESULT}" | head -10
    echo ""
    echo "PUT Performance:"
    grep -A 15 "^### PUT" "${RESULTS_DIR}/${LOCAL_RESULT}" | grep -E "Mean|p50|p95|p99" | head -12
else
    log_warn "No results file found on server"
fi

log_success "SSH-based local testing completed"
