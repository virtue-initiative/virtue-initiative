#!/usr/bin/env bash
# Benchmarks a running hash-server with h2load over plain HTTP, per SPEC.md
# section 4. Requires h2load (nghttp2) on PATH.
#
# Usage:
#   JWT_PRIVATE_KEY_PATH=./dev-private-key.pem ./scripts/bench.sh read [duration_s] [connections] [device_id]
#   JWT_PRIVATE_KEY_PATH=./dev-private-key.pem ./scripts/bench.sh write [duration_s] [connections]
#
# `read` repeatedly calls GET /hash?devices=<id> with a `server` token. This
# is the endpoint's steady-state benchmark: every request is idempotent, so
# throughput reflects real sustained capacity.
#
# `write` repeatedly POSTs a fixed 40-byte body with a `device` token
# for one device. Only the very first request in the run is a durable write
# (201); every request after that is rejected as a 409 sequence conflict
# before touching the database. h2load always repeats the same request body,
# so this can't drive one write per connection the way a real fleet of
# devices would — treat the number it reports as the throughput ceiling for
# the auth + parse + write-queue path, not for sustained disk-durable writes.
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="${1:-read}"
DURATION="${2:-10}"
CONNECTIONS="${3:-50}"
HOST="${HASH_SERVER_URL:-http://127.0.0.1:8788}"
: "${JWT_PRIVATE_KEY_PATH:?set JWT_PRIVATE_KEY_PATH to a PEM file matching the server's JWT_PUBLIC_KEY}"

mint() {
  cargo run --release --quiet --example mint_token -- "$1" "$2" "$JWT_PRIVATE_KEY_PATH"
}

case "$MODE" in
  read)
    DEVICE_ID="${4:-11111111-1111-4111-8111-111111111111}"
    TOKEN=$(mint "unused" "server")
    exec h2load --h1 -D "$DURATION" -c "$CONNECTIONS" \
      -H "Authorization: Bearer $TOKEN" \
      "$HOST/hash?devices=$DEVICE_ID"
    ;;
  write)
    DEVICE_ID="bench-$(date +%s)"
    TOKEN=$(mint "$DEVICE_ID" "device")
    BODY_FILE=$(mktemp)
    trap 'rm -f "$BODY_FILE"' EXIT
    # unix_time=0 (4 LE bytes), seq=1 (4 LE bytes), hash=32 zero bytes.
    { printf '\x00\x00\x00\x00\x01\x00\x00\x00'; head -c 32 /dev/zero; } > "$BODY_FILE"
    exec h2load --h1 -D "$DURATION" -c "$CONNECTIONS" \
      -H "Authorization: Bearer $TOKEN" \
      -H "Content-Type: application/octet-stream" \
      -d "$BODY_FILE" \
      "$HOST/hash"
    ;;
  *)
    echo "unknown mode: $MODE (expected 'read' or 'write')" >&2
    exit 1
    ;;
esac
