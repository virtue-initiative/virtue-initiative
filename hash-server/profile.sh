#!/bin/bash
# Profile hash-server with `perf` while under saturating load, both pinned to
# their isolated cores (same cgroup setup as bench.sh). Produces a perf report
# at /tmp/hash-server-perf.txt.
#
# Usage: ./profile.sh [time_scale] [duration_secs]
set -euo pipefail
cd "$(dirname "$0")"

TS="${1:-6000}"
DURATION="${2:-20}"

rm -f hash-states.db hash-states.db-wal hash-states.db-shm /tmp/hash-server-perf.data

pkill -9 -f 'target/release/hash-server' 2>/dev/null || true
for i in $(seq 1 20); do
    pgrep -f 'target/release/hash-server' >/dev/null || break
    sleep 0.1
done

./target/release/hash-server > /tmp/hash-server.log 2>&1 &
SERVER_PID=$!
sudo bash -c "echo $SERVER_PID > /sys/fs/cgroup/bench/server/cgroup.procs"

for i in $(seq 1 50); do
    if curl -s -o /dev/null -w '' http://localhost:3000/health 2>/dev/null; then
        break
    fi
    sleep 0.1
done

sudo perf record -g --call-graph dwarf -o /tmp/hash-server-perf.data -p "$SERVER_PID" -- sleep "$DURATION" &
PERF_PID=$!

./target/release/examples/loadtest --url http://localhost:3000 --secure-url http://localhost:3000 --users 500 --duration-secs "$DURATION" --time-scale "$TS" &
CLIENT_PID=$!
sudo bash -c "echo $CLIENT_PID > /sys/fs/cgroup/bench/client/cgroup.procs"
wait $CLIENT_PID

wait $PERF_PID

kill -9 $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
pkill -9 -f 'target/release/hash-server' 2>/dev/null || true

sudo chown "$(whoami)" /tmp/hash-server-perf.data
sudo perf report -i /tmp/hash-server-perf.data --stdio --sort=overhead,symbol -n --no-children > /tmp/hash-server-perf.txt
echo "Report written to /tmp/hash-server-perf.txt"
