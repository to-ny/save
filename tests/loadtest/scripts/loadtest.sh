#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

SCENARIO="${1:-mixed}"
CONFIG="${2:-tests/loadtest/config.toml}"
REPORT_NAME="${3:-}"

cd "$PROJECT_ROOT"

if [ ! -f "$CONFIG" ]; then
    echo "Error: Config file $CONFIG not found"
    exit 1
fi

echo "Building save server and loadtest tool..."
cargo build --release -p save-api -p save-loadtest

echo ""
echo "Starting save server..."
SAVE_PID=""
trap 'if [ -n "$SAVE_PID" ]; then kill $SAVE_PID 2>/dev/null || true; fi' EXIT

target/release/save-api &
SAVE_PID=$!

echo "Waiting for server to be ready..."
for i in {1..30}; do
    if curl -s http://localhost:9000/health > /dev/null 2>&1; then
        echo "Server is ready!"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "Error: Server failed to start"
        exit 1
    fi
    sleep 1
done

echo ""
echo "Creating test bucket..."
AWS_ACCESS_KEY_ID=test-access-key AWS_SECRET_ACCESS_KEY=test-secret-key \
    target/release/save --endpoint http://localhost:9000 bucket create loadtest 2>/dev/null || true

echo ""
echo "Running load test scenario: $SCENARIO"

export LOADTEST_CONFIG="$CONFIG"
export LOADTEST_SCENARIO="$SCENARIO"
if [ -n "$REPORT_NAME" ]; then
    export LOADTEST_REPORT_NAME="$REPORT_NAME"
fi

target/release/loadtest

echo ""
echo "Load test complete!"
echo "Results saved to: loadtest-results/"
