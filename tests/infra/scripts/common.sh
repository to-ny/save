#!/bin/bash

set -euo pipefail

readonly RED='\033[0;31m'
readonly YELLOW='\033[1;33m'
readonly GREEN='\033[0;32m'
readonly BLUE='\033[0;34m'
readonly NC='\033[0m'
log_info() {
    echo -e "${BLUE}[INFO]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*" >&2
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $(date '+%Y-%m-%d %H:%M:%S') - $*" >&2
}

check_command() {
    local cmd=$1
    local required=${2:-true}

    if command -v "$cmd" &> /dev/null; then
        return 0
    else
        if [[ "$required" == "true" ]]; then
            log_error "Required command '$cmd' not found. Please install it."
            return 1
        else
            log_warn "Optional command '$cmd' not found."
            return 0
        fi
    fi
}

wait_for_condition() {
    local description=$1
    local max_attempts=$2
    local interval=$3
    shift 3
    local command=("$@")

    log_info "Waiting for: $description"

    local attempt=1
    while [ $attempt -le "$max_attempts" ]; do
        if "${command[@]}" &>/dev/null; then
            log_success "$description (attempt $attempt/$max_attempts)"
            return 0
        fi

        if [ $attempt -lt "$max_attempts" ]; then
            echo -n "."
            sleep "$interval"
        fi
        ((attempt++))
    done

    echo ""
    log_error "Timeout waiting for: $description"
    return 1
}

declare -a TEMP_FILES=()

register_temp_file() {
    TEMP_FILES+=("$1")
}

cleanup_temp_files() {
    if [ ${#TEMP_FILES[@]} -gt 0 ]; then
        log_info "Cleaning up temporary files..."
        for file in "${TEMP_FILES[@]}"; do
            if [ -f "$file" ]; then
                rm -f "$file"
                log_info "Removed: $file"
            fi
        done
    fi
}

handle_error() {
    local line_number=$1
    local command=$2
    log_error "Command failed at line $line_number: $command"
    cleanup_temp_files
}

setup_error_handling() {
    trap 'handle_error ${LINENO} "$BASH_COMMAND"' ERR
    trap cleanup_temp_files EXIT INT TERM
}

get_local_public_ip() {
    local ip=""

    ip=$(curl -s --max-time 5 https://ifconfig.me 2>/dev/null) || \
    ip=$(curl -s --max-time 5 https://api.ipify.org 2>/dev/null) || \
    ip=$(curl -s --max-time 5 https://icanhazip.com 2>/dev/null) || \
    ip=$(curl -s --max-time 5 https://checkip.amazonaws.com 2>/dev/null)

    if [ -n "$ip" ]; then
        echo "$ip"
        return 0
    else
        log_error "Failed to detect public IP address"
        return 1
    fi
}

is_valid_cidr() {
    local cidr=$1
    if [[ $cidr =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}(/[0-9]{1,2})?$ ]]; then
        return 0
    fi
    if [[ $cidr =~ ^([0-9a-fA-F]{0,4}:){2,7}[0-9a-fA-F]{0,4}(/[0-9]{1,3})?$ ]]; then
        return 0
    fi
    return 1
}

get_credential() {
    local var_name=$1
    local prompt_text=$2
    local is_secret=${3:-false}

    if [ -n "${!var_name:-}" ]; then
        echo "${!var_name}"
        return 0
    fi

    if [ "$is_secret" == "true" ]; then
        read -rsp "$prompt_text: " value
        echo "" >&2
    else
        read -rp "$prompt_text: " value
    fi

    echo "$value"
}

require_env_var() {
    local var_name=$1
    local error_msg=${2:-"Required environment variable $var_name is not set"}

    if [ -z "${!var_name:-}" ]; then
        log_error "$error_msg"
        return 1
    fi
}

generate_save_config() {
    local profile=$1
    local access_key=$2
    local secret_key=$3
    local output_file=$4

    local worker_threads
    local write_buffer_mb
    local block_cache_mb

    case "$profile" in
        smoke)
            worker_threads=2
            write_buffer_mb=64
            block_cache_mb=256
            ;;
        medium)
            worker_threads=4
            write_buffer_mb=128
            block_cache_mb=512
            ;;
        large)
            worker_threads=8
            write_buffer_mb=256
            block_cache_mb=2048
            ;;
        *)
            log_error "Unknown profile: $profile"
            return 1
            ;;
    esac

    cat > "$output_file" <<EOF
[server]
bind_address = "0.0.0.0:9000"
worker_threads = $worker_threads
max_blocking_threads = 512

[storage]
data_path = "/var/lib/save/data"
metadata_path = "/var/lib/save/metadata"
fsync_mode = "data"

[metadata]
write_buffer_size_mb = $write_buffer_mb
max_write_buffer_number = 4
block_cache_size_mb = $block_cache_mb
max_background_jobs = 4

[credentials]
access_key = "$access_key"
secret_key = "$secret_key"

[limits]
max_concurrent_requests = 1000
requests_per_second = 500

[shutdown]
drain_timeout_secs = 30
EOF

    log_info "Generated configuration for profile '$profile' at $output_file"
}

run_smoke_tests() {
    local endpoint=$1
    local access_key=$2
    local secret_key=$3

    log_info "Running smoke tests against $endpoint"

    if ! command -v aws &> /dev/null; then
        log_warn "AWS CLI not found, skipping smoke tests"
        return 0
    fi

    local bucket_name="smoke-test-$(date +%s)"
    local test_file="/tmp/smoke-test-data.bin"
    local download_file="/tmp/smoke-test-download.bin"

    register_temp_file "$test_file"
    register_temp_file "$download_file"

    dd if=/dev/urandom of="$test_file" bs=1024 count=1024 &>/dev/null
    local checksum=$(sha256sum "$test_file" | awk '{print $1}')

    export AWS_ACCESS_KEY_ID="$access_key"
    export AWS_SECRET_ACCESS_KEY="$secret_key"
    export AWS_EC2_METADATA_DISABLED=true

    log_info "Creating bucket: $bucket_name"
    if ! aws s3 mb "s3://$bucket_name" --endpoint-url "$endpoint"; then
        log_error "Failed to create bucket"
        return 1
    fi

    log_info "Uploading test object (1MB)"
    if ! aws s3 cp "$test_file" "s3://$bucket_name/test-object" --endpoint-url "$endpoint"; then
        log_error "Failed to upload object"
        return 1
    fi

    log_info "Downloading test object"
    if ! aws s3 cp "s3://$bucket_name/test-object" "$download_file" --endpoint-url "$endpoint"; then
        log_error "Failed to download object"
        return 1
    fi

    log_info "Verifying object integrity"
    local download_checksum=$(sha256sum "$download_file" | awk '{print $1}')
    if [ "$checksum" != "$download_checksum" ]; then
        log_error "Checksum mismatch! Upload: $checksum, Download: $download_checksum"
        return 1
    fi

    log_info "Listing objects"
    if ! aws s3 ls "s3://$bucket_name/" --endpoint-url "$endpoint"; then
        log_error "Failed to list objects"
        return 1
    fi

    log_info "Deleting test object"
    if ! aws s3 rm "s3://$bucket_name/test-object" --endpoint-url "$endpoint"; then
        log_error "Failed to delete object"
        return 1
    fi

    log_info "Deleting bucket"
    if ! aws s3 rb "s3://$bucket_name" --endpoint-url "$endpoint"; then
        log_error "Failed to delete bucket"
        return 1
    fi

    log_success "All smoke tests passed!"
    return 0
}

export -f log_info log_success log_warn log_error
export -f check_command wait_for_condition
export -f get_local_public_ip is_valid_cidr
export -f get_credential require_env_var
export -f generate_save_config run_smoke_tests
export -f cleanup_temp_files handle_error setup_error_handling
export -f register_temp_file
