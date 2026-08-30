#!/bin/bash
# Start all web services (plus the standalone Rust hash server) with
# interleaved colored logs.
# Usage: ./scripts/launch.sh [--donate] [domain]
#   --donate  Also start the donations worker (api-donate) and, if the Stripe
#             CLI is available, forward Stripe webhooks to it.
#   domain    Optional. Registers [domain].localhost and app.[domain].localhost
#             (plus donate.[domain].localhost with --donate) via Caddy reverse
#             proxy (https), mimicking the production URL structure.
#             app.[domain].localhost/api is routed through Vite's /api proxy to wrangler.
#
# Shared configuration (e.g. STRIPE_SECRET_KEY, STRIPE_WEBHOOK_SECRET,
# PUBLIC_STRIPE_PORTAL_URL) can live in ~/.config/virtue-dev.env so it doesn't
# have to be copied into each worker's .dev.vars. That file is sourced below,
# then .env (repo root, per-worktree overrides — see AGENTS.md) is sourced on
# top of it, and the merged values are passed through to the relevant dev
# servers.

DOMAIN=""
DONATE=0
for arg in "$@"; do
    case "$arg" in
        --donate) DONATE=1 ;;
        -h | --help)
            echo "Usage: $0 [--donate] [domain]"
            exit 0
            ;;
        --*)
            echo "Unknown option: $arg" >&2
            echo "Usage: $0 [--donate] [domain]" >&2
            exit 1
            ;;
        *) DOMAIN="$arg" ;;
    esac
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Shared dev configuration. Anything exported here (secrets, portal URLs, etc.)
# is available to the child dev servers without editing per-worker .dev.vars.
# .env (repo root) is sourced second so its values win over the machine-wide file.
VIRTUE_DEV_ENV="${VIRTUE_DEV_ENV:-$HOME/.config/virtue-dev.env}"
if [ -f "$VIRTUE_DEV_ENV" ]; then
    set -a
    # shellcheck disable=SC1090
    . "$VIRTUE_DEV_ENV"
    set +a
fi
if [ -f "$ROOT/.env" ]; then
    set -a
    # shellcheck disable=SC1091
    . "$ROOT/.env"
    set +a
fi

# api/.dev.vars keys that can be centrally overridden (see AGENTS.md); passed
# as --var so they win over api/.dev.vars without editing that file.
API_EXTRA_VARS=()
for _key in JWT_PRIVATE_KEY JWT_PUBLIC_KEY APP_NAME API_BASE_PATH EMAIL_DELIVERY_MODE BUG_REPORT_EMAIL AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_REGION; do
    eval "_value=\${$_key:-}"
    [ -n "$_value" ] && API_EXTRA_VARS+=(--var "$_key:$_value")
done
unset _key _value

# Pick 5 unique free ports in one bun process so the OS can't reuse them between calls.
read -r API_PORT WEB_PORT LANDING_PORT DONATE_PORT HASH_PORT < <(bun -e "
const {createServer} = require('net');
const pick = () => new Promise(r => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); });
});
Promise.all([pick(), pick(), pick(), pick(), pick()]).then(ports => console.log(ports.join(' ')));
" 2>/dev/null)

# The hash server verifies JWTs signed by the API, so it needs the same public
# key. Pull it straight out of api/.dev.vars (setup.sh's copy of
# api/.dev.vars.example) rather than hardcoding it, so a locally rotated
# keypair stays in sync automatically.
API_DEV_VARS="$ROOT/api/.dev.vars"
if [ ! -f "$API_DEV_VARS" ]; then
    echo "Error: $API_DEV_VARS not found. Run ./scripts/setup.sh first." >&2
    exit 1
fi
HASH_SERVER_JWT_PUBLIC_KEY="$(grep -m1 '^JWT_PUBLIC_KEY=' "$API_DEV_VARS" | sed -E 's/^JWT_PUBLIC_KEY="?//; s/"$//')"
if [ -z "$HASH_SERVER_JWT_PUBLIC_KEY" ]; then
    echo "Error: could not read JWT_PUBLIC_KEY from $API_DEV_VARS." >&2
    exit 1
fi

export FORCE_COLOR=1

cleanup() {
    trap '' INT TERM
    kill 0 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup INT TERM EXIT

run() {
    local name="$1" color="$2" dir="$3"
    shift 3
    (cd "$ROOT/$dir" && "$@") 2>&1 \
        | while IFS= read -r line; do
            printf '\033[%sm[%s]\033[0m %s\n' "$color" "$name" "$line"
          done &
}

# Start the standalone hash server (see hash-server/SPEC.md). Always run, since
# both the API and client rely on it for the hash-chain protocol.
start_hash_server() {
    run "hash" "33" "hash-server" env \
        HOST=127.0.0.1 \
        PORT="$HASH_PORT" \
        RUST_LOG=info \
        "JWT_PUBLIC_KEY=$HASH_SERVER_JWT_PUBLIC_KEY" \
        cargo run --quiet
}

# Local D1 state persists across dev sessions, but this script picks a fresh
# random API port every run, which gets baked into R2_URL (local batch urls
# route through the api worker's own /r2 proxy, not a real R2 domain). Without
# this, batches uploaded in a previous session point at a dead port. Rewrite
# their stored urls to the current R2_URL so old local batches keep resolving.
rewrite_local_batch_urls() {
    local new_r2_url="$1"
    (cd "$ROOT/api" && bun run wrangler d1 execute staging-app-db --local --env staging \
        --command "UPDATE batches SET url = '${new_r2_url}/' || substr(url, instr(url, '/r2/') + 4) WHERE url LIKE '%/r2/%';" \
        > /dev/null 2>&1) || true
}

# Start the donations worker (and Stripe webhook forwarder). $1 is the landing
# URL the worker should use for CORS and Stripe redirect URLs.
start_donate() {
    local landing_url="$1"
    local webhook_url="http://localhost:${DONATE_PORT}/webhook"
    local donate_vars=()

    [ -n "$STRIPE_SECRET_KEY" ] && donate_vars+=(--var "STRIPE_SECRET_KEY:${STRIPE_SECRET_KEY}")

    if command -v stripe > /dev/null 2>&1; then
        # `stripe listen` signs forwarded events with this secret; grab it so the
        # worker can verify them, then start the forwarder.
        local secret
        secret="$(stripe listen --print-secret 2>/dev/null)"
        if [ -n "$secret" ]; then
            STRIPE_WEBHOOK_SECRET="$secret"
            run "stripe" "36" "." stripe listen --forward-to "$webhook_url"
        else
            echo "Warning: could not read Stripe webhook secret (try 'stripe login'); webhook forwarding disabled." >&2
        fi
    else
        echo "Warning: stripe CLI not found; webhook forwarding disabled." >&2
    fi

    [ -n "$STRIPE_WEBHOOK_SECRET" ] && donate_vars+=(--var "STRIPE_WEBHOOK_SECRET:${STRIPE_WEBHOOK_SECRET}")

    run "donate" "35" "api-donate" bun run dev -- --port "$DONATE_PORT" \
        --var "LANDING_URL:${landing_url}" "${donate_vars[@]}"
}

if [ -n "$DOMAIN" ]; then
    if ! curl -sf http://localhost:2019/config/ > /dev/null 2>&1; then
        echo "Error: Caddy is not running. Run ./scripts/setup.sh first." >&2
        exit 1
    fi

    CADDY_API="http://localhost:2019"
    WEB_ROUTE_ID="virtue-web-${DOMAIN}"
    LANDING_ROUTE_ID="virtue-landing-${DOMAIN}"
    DONATE_ROUTE_ID="virtue-donate-${DOMAIN}"

    export VIRTUE_DEV_CA_CERT="$HOME/.local/share/caddy/pki/authorities/local/root.crt"
    export NODE_EXTRA_CA_CERTS="$HOME/.local/share/caddy/pki/authorities/local/root.crt"

    # Remove any stale routes from a previous run (donate deletes are harmless
    # even when it wasn't started, and clear routes left by an earlier --donate run).
    curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${DONATE_ROUTE_ID}" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}-http" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}-http" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${DONATE_ROUTE_ID}-http" > /dev/null 2>&1 || true

    # Register HTTPS routes: app.DOMAIN.localhost → web, DOMAIN.localhost → landing
    curl -sf -X POST "${CADDY_API}/config/apps/http/servers/dev/routes" \
        -H "Content-Type: application/json" \
        -d "{\"@id\":\"${WEB_ROUTE_ID}\",\"match\":[{\"host\":[\"app.${DOMAIN}.localhost\"]}],\"handle\":[{\"handler\":\"reverse_proxy\",\"upstreams\":[{\"dial\":\"127.0.0.1:${WEB_PORT}\"}]}]}"

    curl -sf -X POST "${CADDY_API}/config/apps/http/servers/dev/routes" \
        -H "Content-Type: application/json" \
        -d "{\"@id\":\"${LANDING_ROUTE_ID}\",\"match\":[{\"host\":[\"${DOMAIN}.localhost\"]}],\"handle\":[{\"handler\":\"reverse_proxy\",\"upstreams\":[{\"dial\":\"127.0.0.1:${LANDING_PORT}\"}]}]}"

    # Register HTTP routes
    curl -sf -X POST "${CADDY_API}/config/apps/http/servers/dev-http/routes" \
        -H "Content-Type: application/json" \
        -d "{\"@id\":\"${WEB_ROUTE_ID}-http\",\"match\":[{\"host\":[\"app.${DOMAIN}.localhost\"]}],\"handle\":[{\"handler\":\"reverse_proxy\",\"upstreams\":[{\"dial\":\"127.0.0.1:${WEB_PORT}\"}]}]}"

    curl -sf -X POST "${CADDY_API}/config/apps/http/servers/dev-http/routes" \
        -H "Content-Type: application/json" \
        -d "{\"@id\":\"${LANDING_ROUTE_ID}-http\",\"match\":[{\"host\":[\"${DOMAIN}.localhost\"]}],\"handle\":[{\"handler\":\"reverse_proxy\",\"upstreams\":[{\"dial\":\"127.0.0.1:${LANDING_PORT}\"}]}]}"

    ALLOWED_HOSTS="app.${DOMAIN}.localhost,${DOMAIN}.localhost"

    if [ "$DONATE" = 1 ]; then
        curl -sf -X POST "${CADDY_API}/config/apps/http/servers/dev/routes" \
            -H "Content-Type: application/json" \
            -d "{\"@id\":\"${DONATE_ROUTE_ID}\",\"match\":[{\"host\":[\"donate.${DOMAIN}.localhost\"]}],\"handle\":[{\"handler\":\"reverse_proxy\",\"upstreams\":[{\"dial\":\"127.0.0.1:${DONATE_PORT}\"}]}]}"
        curl -sf -X POST "${CADDY_API}/config/apps/http/servers/dev-http/routes" \
            -H "Content-Type: application/json" \
            -d "{\"@id\":\"${DONATE_ROUTE_ID}-http\",\"match\":[{\"host\":[\"donate.${DOMAIN}.localhost\"]}],\"handle\":[{\"handler\":\"reverse_proxy\",\"upstreams\":[{\"dial\":\"127.0.0.1:${DONATE_PORT}\"}]}]}"
        ALLOWED_HOSTS="${ALLOWED_HOSTS},donate.${DOMAIN}.localhost"
        export PUBLIC_DONATE_API_URL="https://donate.${DOMAIN}.localhost"
    fi

    export VITE_API_URL="https://app.${DOMAIN}.localhost/api"
    export VITE_API_PROXY_TARGET="http://localhost:${API_PORT}"
    export VITE_LANDING_URL="https://${DOMAIN}.localhost"
    export __VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS="$ALLOWED_HOSTS"
    export PUBLIC_APP_URL="https://app.${DOMAIN}.localhost"
    export PUBLIC_API_URL="https://app.${DOMAIN}.localhost/api"
    printf '\n  Landing : http://%s.localhost  /  https://%s.localhost\n' "$DOMAIN" "$DOMAIN"
    printf '  Web     : http://app.%s.localhost  /  https://app.%s.localhost\n' "$DOMAIN" "$DOMAIN"
    printf '  API     : https://app.%s.localhost/api\n' "$DOMAIN"
    [ "$DONATE" = 1 ] && printf '  Donate  : https://donate.%s.localhost\n' "$DOMAIN"
    printf '  Hash    : http://localhost:%s\n' "$HASH_PORT"
    printf '\n'

    cleanup() {
        trap '' INT TERM
        curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${DONATE_ROUTE_ID}" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}-http" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}-http" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${DONATE_ROUTE_ID}-http" > /dev/null 2>&1 || true
        kill 0 2>/dev/null || true
        wait 2>/dev/null || true
    }

    rewrite_local_batch_urls "https://app.${DOMAIN}.localhost/r2"
    start_hash_server
    run "api"     "31" "api"     bun run dev -- --port "$API_PORT" \
        --var "APP_URL:https://app.${DOMAIN}.localhost" \
        --var "LANDING_URL:https://${DOMAIN}.localhost" \
        --var "R2_URL:https://app.${DOMAIN}.localhost/r2" \
        --var "HASH_SERVER_URL:http://localhost:${HASH_PORT}" \
        "${API_EXTRA_VARS[@]}"
    [ "$DONATE" = 1 ] && start_donate "https://${DOMAIN}.localhost"
    run "web"     "32" "web"     bun run dev -- --port "$WEB_PORT" --host 127.0.0.1
    run "landing" "34" "landing" bun run dev -- --port "$LANDING_PORT" --host 127.0.0.1
else
    export VITE_API_URL="http://localhost:${API_PORT}"
    export VITE_API_PROXY_TARGET="http://localhost:${API_PORT}"
    export VITE_LANDING_URL="http://localhost:${LANDING_PORT}"
    export PUBLIC_APP_URL="http://localhost:${WEB_PORT}"
    export PUBLIC_API_URL="http://localhost:${API_PORT}"
    [ "$DONATE" = 1 ] && export PUBLIC_DONATE_API_URL="http://localhost:${DONATE_PORT}"
    printf '\n  Landing : http://localhost:%s\n' "$LANDING_PORT"
    printf '  Web     : http://localhost:%s\n' "$WEB_PORT"
    printf '  API     : http://localhost:%s\n' "$API_PORT"
    [ "$DONATE" = 1 ] && printf '  Donate  : http://localhost:%s\n' "$DONATE_PORT"
    printf '  Hash    : http://localhost:%s\n' "$HASH_PORT"
    printf '\n'

    rewrite_local_batch_urls "http://localhost:${API_PORT}/r2"
    start_hash_server
    run "api"     "31" "api"     bun run dev -- --port "$API_PORT" \
        --var "APP_URL:http://localhost:${WEB_PORT}" \
        --var "LANDING_URL:http://localhost:${LANDING_PORT}" \
        --var "R2_URL:http://localhost:${API_PORT}/r2" \
        --var "HASH_SERVER_URL:http://localhost:${HASH_PORT}" \
        "${API_EXTRA_VARS[@]}"
    [ "$DONATE" = 1 ] && start_donate "http://localhost:${LANDING_PORT}"
    run "web"     "32" "web"     bun run dev -- --port "$WEB_PORT"
    run "landing" "34" "landing" bun run dev -- --port "$LANDING_PORT"
fi

wait
