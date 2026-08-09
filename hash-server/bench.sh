#!/bin/bash
# Run one hash-server load-test point, with the server pinned to isolated
# core 6 and the loadtest client pinned to isolated core 7 (cgroup v2
# "bench" partition set up separately). Prints the loadtest summary table.
#
# Usage: ./bench.sh <users> <duration_secs> [extra loadtest args...]
set -euo pipefail
cd "$(dirname "$0")"

USERS="${1:?users required}"
DURATION="${2:?duration_secs required}"
shift 2

rm -f hash-states.db hash-states.db-wal hash-states.db-shm

# Clean up any stale hash-server from a previous run that didn't die cleanly.
pkill -9 -f 'target/release/hash-server' 2>/dev/null || true
for i in $(seq 1 20); do
    pgrep -f 'target/release/hash-server' >/dev/null || break
    sleep 0.1
done

./target/release/hash-server > /tmp/hash-server.log 2>&1 &
SERVER_PID=$!
sudo bash -c "echo $SERVER_PID > /sys/fs/cgroup/bench/server/cgroup.procs"

# wait for health
for i in $(seq 1 50); do
    if curl -s -o /dev/null -w '' http://localhost:3000/health 2>/dev/null; then
        break
    fi
    sleep 0.1
done

./target/release/examples/loadtest --url http://localhost:3000 --secure-url http://localhost:3000 --users "$USERS" --duration-secs "$DURATION" "$@" &
CLIENT_PID=$!
sudo bash -c "echo $CLIENT_PID > /sys/fs/cgroup/bench/client/cgroup.procs"
wait $CLIENT_PID

kill -9 $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
pkill -9 -f 'target/release/hash-server' 2>/dev/null || true
