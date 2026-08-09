#!/bin/bash
# Load-test the hash-server running on the remote Oracle Cloud box, through an
# SSH tunnel (localhost:3001 -> remote:3000, set up separately). The loadtest
# client runs locally, pinned to the isolated core (cgroup "bench/client"),
# same as bench.sh, so this machine's own noise doesn't skew results.
#
# Usage: ./bench-remote.sh <users> <duration_secs> [extra loadtest args...]
set -euo pipefail
cd "$(dirname "$0")"

USERS="${1:?users required}"
DURATION="${2:?duration_secs required}"
shift 2

./target/release/examples/loadtest --url http://localhost:3001 --users "$USERS" --duration-secs "$DURATION" "$@" &
CLIENT_PID=$!
sudo bash -c "echo $CLIENT_PID > /sys/fs/cgroup/bench/client/cgroup.procs"
wait $CLIENT_PID
