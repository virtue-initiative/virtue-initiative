#!/usr/bin/env bash
# Device -> api/hash-server integration smoke test (macOS).
#
# Boots the api worker locally against a fresh D1 database (the api's own
# D1-backed /hash routes stand in for the standalone Rust hash-server in
# local dev -- see api/src/lib/hash-server.ts and scripts/launch.sh), seeds
# the deterministic dev account, builds and runs the real virtue-mac daemon
# binary directly (no launchd, no packaged .app), logs it in over its IPC
# socket, waits for a real screenshot/hash/batch cycle, then asserts that
# hashes and batches actually landed in the database.
#
# Screen Recording permission: CI runners don't have it granted, and macOS's
# TCC framework has no scriptable/headless way to grant it (unlike Linux's
# Xvfb, which just gives the daemon a permission-free virtual display).
# Rather than testing the CaptureFailed alert fallback instead of a real
# capture, the daemon here is built with the mock-capture feature
# (client/mac/src/capture.rs), which swaps in a fixed embedded PNG in place
# of shelling out to `screencapture`. That's compiled in only when this
# script explicitly requests it -- never by build-app.sh/build-dmg.sh -- so
# it can't end up in a shipped build, and it still exercises the real
# capture -> classify -> upload -> hash -> batch pipeline end to end -- it
# just doesn't cover the Screen Recording permission-gating logic itself
# (CGPreflightScreenCaptureAccess / CaptureFailed), which stays untested by
# this job.
#
# Usage: ./client/mac/scripts/integration-test.sh
#
# Requires: bun, cargo, curl, all on PATH. macOS only.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
CLIENT_DIR="$ROOT/client"
API_DIR="$ROOT/api"

DEV_EMAIL="dev@dev.com"
DEV_PASSWORD="devpassword"
DEVICE_NAME="ci-integration-test-$$"

# capture_interval_seconds has a 15s floor enforced by client/core/src/config.rs.
CAPTURE_INTERVAL_SECONDS=15
BATCH_WINDOW_SECONDS=15
# One capture interval for the (mocked) screenshot to fire, plus one batch
# window for it to flush, plus margin for CI scheduling jitter.
RUN_DURATION_SECONDS=45

if [ "$(uname -s)" != "Darwin" ]; then
  echo "integration-test: this script only runs on macOS" >&2
  exit 1
fi

for cmd in bun cargo curl; do
  if ! command -v "$cmd" > /dev/null 2>&1; then
    echo "integration-test: missing required command '$cmd' on PATH" >&2
    exit 1
  fi
done

LOG_DIR="$(mktemp -d)"
API_LOG="$LOG_DIR/api.log"
DAEMON_LOG="$LOG_DIR/daemon.log"

# Isolated HOME for the client under test only -- NOT exported globally. On
# macOS `dirs::config_dir()`/`data_dir()`/`home_dir()` all resolve off $HOME,
# so overriding it isolates ~/Library/Application Support/virtue and
# ~/Library/LaunchAgents the same way Linux isolates XDG_CONFIG_HOME/
# XDG_STATE_HOME -- without touching a real local `virtue` install.
#
# Deliberately rooted at /tmp rather than plain `mktemp -d` (which resolves
# under $TMPDIR, e.g. /var/folders/xx/xxxxxxxxxxxxxxxxxxxxxxxxxxxx/T on
# macOS): daemon.sock's full path must fit in sockaddr_un.sun_path, capped at
# 104 bytes on macOS, and "$TMPDIR/.../Library/Application Support/virtue/
# state/daemon.sock" alone already exceeds that -- IpcBridge::bind() then
# fails, and since IPC is treated as optional (the daemon still runs without
# a controller connection), it fails silently with no error in the daemon
# log, just a socket that never appears.
TMP_HOME="$(mktemp -d /tmp/virtue-mac-ci.XXXXXX)"
CLIENT_APP_SUPPORT="$TMP_HOME/Library/Application Support/virtue"

API_PID=""
DAEMON_PID=""

cleanup() {
  local status=$?
  set +e
  if [ -n "$DAEMON_PID" ]; then
    kill "$DAEMON_PID" 2>/dev/null
    wait "$DAEMON_PID" 2>/dev/null
  fi
  if [ -n "$API_PID" ]; then kill "$API_PID" 2>/dev/null; wait "$API_PID" 2>/dev/null; fi
  if [ "$status" -ne 0 ]; then
    echo "=== api log ==="
    cat "$API_LOG" 2>/dev/null
    echo "=== daemon log ==="
    cat "$DAEMON_LOG" 2>/dev/null
  fi
  rm -rf "$TMP_HOME" "$LOG_DIR"
  exit "$status"
}
trap cleanup EXIT INT TERM

echo "== Picking a free port for the api dev server =="
API_PORT="$(bun -e "
const {createServer} = require('net');
const s = createServer();
s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => console.log(p)); });
")"
API_BASE_URL="http://localhost:${API_PORT}"

echo "== Setting up api/ local dev environment (port ${API_PORT}) =="
(
  cd "$API_DIR"
  [ -f .dev.vars ] || cp .dev.vars.example .dev.vars
  [ -d node_modules ] || bun install
  bun run db:migrate:local
)

echo "== Starting api dev server =="
(
  cd "$API_DIR"
  exec bun run dev -- --port "$API_PORT" --var "HASH_SERVER_URL:${API_BASE_URL}/api"
) > "$API_LOG" 2>&1 &
API_PID=$!

echo "== Waiting for api dev server to become ready =="
ready=0
for _ in $(seq 1 60); do
  if curl -sf "$API_BASE_URL/" > /dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [ "$ready" -ne 1 ]; then
  echo "integration-test: api dev server did not become ready in time" >&2
  exit 1
fi

echo "== Seeding dev user =="
bun run "$ROOT/scripts/seed-dev-user.mjs"

echo "== Building virtue-mac client (mock-capture) =="
(cd "$CLIENT_DIR" && cargo build -p virtue-mac --features mock-capture)
VIRTUE_BIN="$CLIENT_DIR/target/debug/virtue-mac"
CI_LOGIN_BIN="$CLIENT_DIR/target/debug/virtue-mac-ci-login"

echo "== Writing isolated client config =="
mkdir -p "$CLIENT_APP_SUPPORT"
cat > "$CLIENT_APP_SUPPORT/config.json" <<EOF
{
  "api_base_url": "${API_BASE_URL}",
  "capture_interval_seconds": ${CAPTURE_INTERVAL_SECONDS},
  "batch_window_seconds": ${BATCH_WINDOW_SECONDS}
}
EOF

echo "== Starting the daemon =="
env HOME="$TMP_HOME" "$VIRTUE_BIN" daemon > "$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

echo "== Waiting for the daemon IPC socket =="
DAEMON_SOCK="$CLIENT_APP_SUPPORT/state/daemon.sock"
daemon_ready=0
for _ in $(seq 1 30); do
  if [ -S "$DAEMON_SOCK" ]; then
    daemon_ready=1
    break
  fi
  sleep 1
done
if [ "$daemon_ready" -ne 1 ]; then
  echo "integration-test: daemon did not create its IPC socket in time" >&2
  exit 1
fi

echo "== Logging in =="
"$CI_LOGIN_BIN" \
  --socket "$DAEMON_SOCK" \
  --email "$DEV_EMAIL" \
  --password "$DEV_PASSWORD" \
  --device-name "$DEVICE_NAME"

echo "== Waiting ${RUN_DURATION_SECONDS}s for capture/batch/hash activity =="
sleep "$RUN_DURATION_SECONDS"

echo "== Verifying database state =="

d1_query_count() {
  local sql="$1"
  (
    cd "$API_DIR"
    bun run wrangler d1 execute staging-app-db --local --env staging --json --command "$sql"
  ) | bun -e '
    const data = JSON.parse(await Bun.stdin.text());
    console.log(data[0]?.results?.[0]?.c ?? 0);
  '
}

# hash_states.count is a rolling per-batch-window counter, not a cumulative
# total: api/src/routes/device-only.ts resets it to 0 after every successful
# POST /d/batch (see hashReset() there), so with our short batch window it
# can legitimately read 0 moments after a hash was ingested. hashed_at is
# never touched by that reset (see localHashReset in api/src/lib/hash-server.ts),
# so it's the durable signal that at least one hash was ever ingested.
#
# A `wrangler d1 execute --local` CLI process reading the same on-disk D1
# state can also lag slightly behind Miniflare's in-process view right after
# a write, so retry for a few seconds instead of asserting on one snapshot.
fail=1
for _ in $(seq 1 15); do
  DEVICE_COUNT="$(d1_query_count "SELECT COUNT(*) as c FROM devices WHERE name = '${DEVICE_NAME}'")"
  HASH_COUNT="$(d1_query_count "SELECT COUNT(*) as c FROM hash_states hs JOIN devices d ON d.id = hs.device_id WHERE d.name = '${DEVICE_NAME}' AND hs.hashed_at IS NOT NULL")"
  BATCH_COUNT="$(d1_query_count "SELECT COUNT(*) as c FROM batches b JOIN devices d ON d.id = b.device_id WHERE d.name = '${DEVICE_NAME}'")"

  echo "device rows: ${DEVICE_COUNT}, ever-hashed: ${HASH_COUNT}, batch rows: ${BATCH_COUNT}"

  if [ "$DEVICE_COUNT" -ge 1 ] && [ "$HASH_COUNT" -ge 1 ] && [ "$BATCH_COUNT" -ge 1 ]; then
    fail=0
    break
  fi
  sleep 2
done

if [ "$fail" -ne 0 ]; then
  [ "$DEVICE_COUNT" -ge 1 ] || echo "integration-test: expected a devices row for '${DEVICE_NAME}'" >&2
  [ "$HASH_COUNT" -ge 1 ] || echo "integration-test: expected a hash_states row with hashed_at set for '${DEVICE_NAME}'" >&2
  [ "$BATCH_COUNT" -ge 1 ] || echo "integration-test: expected at least one batch row for '${DEVICE_NAME}'" >&2

  echo "--- devices (raw) ---"
  (cd "$API_DIR" && bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(id) as id, name FROM devices")
  echo "--- hash_states (raw) ---"
  (cd "$API_DIR" && bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(device_id) as device_id, count, hashed_at FROM hash_states")
  echo "--- batches (raw) ---"
  (cd "$API_DIR" && bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(device_id) as device_id, COUNT(*) as n FROM batches GROUP BY device_id")
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi

echo "== Integration test passed =="
