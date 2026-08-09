#!/bin/bash
# Sweep aggregate request rate against the remote Oracle Cloud hash-server
# (via bench-remote.sh / SSH tunnel), fixed devices=1000 (users=500 x
# devices-per-user=2). Prints remote load average after each point so we can
# bail before overloading the small 2-OCPU box.
set -euo pipefail
cd "$(dirname "$0")"

DURATION="${1:-20}"
shift || true

echo "time_scale  target_req/s"
for ts in "$@"; do
    target=$(awk "BEGIN{printf \"%.0f\", 1000*$ts/300}")
    echo "=== time_scale=$ts  (target ~${target} req/s)  duration=${DURATION}s ==="
    ./bench-remote.sh 500 "$DURATION" --time-scale "$ts"
    ssh -o BatchMode=yes -o ConnectTimeout=10 ubuntu@129.213.88.104 "echo -n 'remote load: '; uptime | sed -E 's/.*load average: //'"
    echo
done
