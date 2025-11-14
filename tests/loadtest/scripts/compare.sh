#!/usr/bin/env bash
set -euo pipefail

RESULTS_DIR="${1:-loadtest-results}"
BASELINE="${2:-}"

cd "$RESULTS_DIR"

if [ ! -d "$RESULTS_DIR" ]; then
    echo "Error: Results directory $RESULTS_DIR not found"
    exit 1
fi

JSON_FILES=($(ls -t *.json 2>/dev/null || true))

if [ ${#JSON_FILES[@]} -eq 0 ]; then
    echo "No test results found in $RESULTS_DIR"
    exit 1
fi

echo "Historical Performance Comparison"
echo "=================================="
echo ""
echo "| Date | Test | Req/s | p50 (ms) | p95 (ms) | p99 (ms) | Success Rate |"
echo "|------|------|-------|----------|----------|----------|--------------|"

for file in "${JSON_FILES[@]}"; do
    if command -v jq &> /dev/null; then
        TEST_NAME=$(jq -r '.test_name' "$file")
        START=$(jq -r '.start_time' "$file" | cut -d'T' -f1)
        RPS=$(jq -r '.summary.requests_per_second' "$file")
        P50=$(jq -r '.summary.latency_p50_ms' "$file")
        P95=$(jq -r '.summary.latency_p95_ms' "$file")
        P99=$(jq -r '.summary.latency_p99_ms' "$file")
        TOTAL=$(jq -r '.summary.total_requests' "$file")
        SUCCESS=$(jq -r '.summary.successful_requests' "$file")
        SUCCESS_RATE=$(echo "scale=2; $SUCCESS * 100 / $TOTAL" | bc)

        printf "| %s | %s | %.2f | %.2f | %.2f | %.2f | %.2f%% |\n" \
            "$START" "$TEST_NAME" "$RPS" "$P50" "$P95" "$P99" "$SUCCESS_RATE"
    else
        echo "Warning: jq not found, skipping $file"
    fi
done
