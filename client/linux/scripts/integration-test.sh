#!/usr/bin/env bash
# Device -> api/hash-server integration smoke test (Linux).
#
# Boots the api worker locally against a fresh D1 database (the api's own
# D1-backed /hash routes stand in for the standalone Rust hash-server in
# local dev -- see api/src/lib/hash-server.ts and scripts/launch.sh), seeds
# the deterministic dev account, builds and runs the real virtue-linux
# daemon under Xvfb (so screenshot capture produces a genuine, if black,
# screenshot with no mocking code), logs in, lets it run for a short window,
# then asserts that hashes and batches actually landed in the database.
#
# Usage: ./client/linux/scripts/integration-test.sh
#
# Requires: bun, cargo, curl, xvfb-run, and the `import` tool from
# imagemagick, all on PATH.

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
RUN_DURATION_SECONDS=60

for cmd in bun cargo curl xvfb-run import; do
  if ! command -v "$cmd" > /dev/null 2>&1; then
    echo "integration-test: missing required command '$cmd' on PATH" >&2
    exit 1
  fi
done

LOG_DIR="$(mktemp -d)"
API_LOG="$LOG_DIR/api.log"
DAEMON_LOG="$LOG_DIR/daemon.log"

# Isolated home for the client under test only -- NOT exported globally.
# rustup resolves its default toolchain from $HOME/.rustup at runtime (unlike
# $CARGO_HOME, which the Setup Rust step pins explicitly), so swapping HOME
# for the whole script breaks `cargo build`. Only the daemon and login
# processes below get this HOME/XDG override.
TMP_HOME="$(mktemp -d)"
CLIENT_XDG_CONFIG_HOME="$TMP_HOME/config"
CLIENT_XDG_STATE_HOME="$TMP_HOME/state"

API_PID=""
DAEMON_PID=""

cleanup() {
  local status=$?
  set +e
  if [ -n "$DAEMON_PID" ]; then
    kill "$DAEMON_PID" 2>/dev/null
    wait "$DAEMON_PID" 2>/dev/null
    # xvfb-run doesn't reliably forward TERM to the command it wraps, so the
    # daemon (and its Xvfb server) can otherwise survive as an orphan.
    pkill -f "Xvfb :" 2>/dev/null
    pkill -f "$VIRTUE_BIN daemon" 2>/dev/null
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

echo "== Building virtue-linux client =="
(cd "$CLIENT_DIR" && cargo build -p virtue-linux)
VIRTUE_BIN="$CLIENT_DIR/target/debug/virtue"

echo "== Writing isolated client config =="
mkdir -p "$CLIENT_XDG_CONFIG_HOME/virtue"
cat > "$CLIENT_XDG_CONFIG_HOME/virtue/config.json" <<EOF
{
  "api_base_url": "${API_BASE_URL}",
  "capture_interval_seconds": ${CAPTURE_INTERVAL_SECONDS},
  "batch_window_seconds": ${BATCH_WINDOW_SECONDS}
}
EOF

echo "== Starting the daemon under Xvfb =="
env HOME="$TMP_HOME" XDG_CONFIG_HOME="$CLIENT_XDG_CONFIG_HOME" XDG_STATE_HOME="$CLIENT_XDG_STATE_HOME" \
  xvfb-run -a "$VIRTUE_BIN" daemon > "$DAEMON_LOG" 2>&1 &
DAEMON_PID=$!

echo "== Waiting for the daemon IPC socket =="
DAEMON_SOCK="$CLIENT_XDG_STATE_HOME/virtue/daemon.sock"
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
env HOME="$TMP_HOME" XDG_CONFIG_HOME="$CLIENT_XDG_CONFIG_HOME" XDG_STATE_HOME="$CLIENT_XDG_STATE_HOME" \
  bun "$SCRIPT_DIR/ci-login.ts" \
  --bin "$VIRTUE_BIN" \
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

# Each POST /hash or /d/batch response returns only once its D1 write has been
# awaited, but a separate `wrangler d1 execute --local` CLI process reading the
# same on-disk D1 state can still lag slightly behind Miniflare's in-process
# view -- so retry for a few seconds instead of asserting on a single snapshot.
fail=1
for _ in $(seq 1 15); do
  DEVICE_COUNT="$(d1_query_count "SELECT COUNT(*) as c FROM devices WHERE name = '${DEVICE_NAME}'")"
  HASH_COUNT="$(d1_query_count "SELECT COALESCE((SELECT hs.count FROM hash_states hs JOIN devices d ON d.id = hs.device_id WHERE d.name = '${DEVICE_NAME}'), 0) as c")"
  BATCH_COUNT="$(d1_query_count "SELECT COUNT(*) as c FROM batches b JOIN devices d ON d.id = b.device_id WHERE d.name = '${DEVICE_NAME}'")"

  echo "device rows: ${DEVICE_COUNT}, hash count: ${HASH_COUNT}, batch rows: ${BATCH_COUNT}"

  if [ "$DEVICE_COUNT" -ge 1 ] && [ "$HASH_COUNT" -ge 1 ] && [ "$BATCH_COUNT" -ge 1 ]; then
    fail=0
    break
  fi
  sleep 2
done

if [ "$fail" -ne 0 ]; then
  [ "$DEVICE_COUNT" -ge 1 ] || echo "integration-test: expected a devices row for '${DEVICE_NAME}'" >&2
  [ "$HASH_COUNT" -ge 1 ] || echo "integration-test: expected hash_states.count > 0 for '${DEVICE_NAME}'" >&2
  [ "$BATCH_COUNT" -ge 1 ] || echo "integration-test: expected at least one batch row for '${DEVICE_NAME}'" >&2

  echo "--- devices (raw) ---"
  (cd "$API_DIR" && bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(id) as id, name FROM devices")
  echo "--- hash_states (raw) ---"
  (cd "$API_DIR" && bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(device_id) as device_id, count FROM hash_states")
  echo "--- batches (raw) ---"
  (cd "$API_DIR" && bun run wrangler d1 execute staging-app-db --local --env staging --command "SELECT hex(device_id) as device_id, COUNT(*) as n FROM batches GROUP BY device_id")
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi

echo "== Integration test passed =="
