#!/bin/bash
# Sweep aggregate request rate (via --time-scale, fixed device count) to find
# where the isolated single-core hash-server starts erroring or its p99
# blows out. Fixed devices=1000 (users=500 x devices-per-user=2); time-scale
# controls how compressed the real-world cadence is, which linearly controls
# aggregate req/s: req/s ~= 1000 * time_scale / 300.
set -euo pipefail
cd "$(dirname "$0")"

DURATION="${1:-20}"
shift || true

echo "time_scale  target_req/s"
for ts in "$@"; do
    target=$(awk "BEGIN{printf \"%.0f\", 1000*$ts/300}")
    echo "=== time_scale=$ts  (target ~${target} req/s)  duration=${DURATION}s ==="
    ./bench.sh 500 "$DURATION" --time-scale "$ts"
    echo
done
