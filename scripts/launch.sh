#!/bin/bash
# Start all web services with interleaved colored logs.
# Usage: ./scripts/launch.sh [domain]
#   domain  Optional. Registers [domain].localhost and app.[domain].localhost
#           via Caddy reverse proxy (https), mimicking the production URL structure.
#           app.[domain].localhost/api is routed through Vite's /api proxy to wrangler.

DOMAIN="${1:-}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Pick 3 unique free ports in one bun process so the OS can't reuse them between calls.
read -r API_PORT WEB_PORT LANDING_PORT < <(bun -e "
const {createServer} = require('net');
const pick = () => new Promise(r => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); });
});
Promise.all([pick(), pick(), pick()]).then(([a, b, c]) => console.log(a, b, c));
" 2>/dev/null)

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

if [ -n "$DOMAIN" ]; then
    if ! curl -sf http://localhost:2019/config/ > /dev/null 2>&1; then
        echo "Error: Caddy is not running. Run ./scripts/setup.sh first." >&2
        exit 1
    fi

    CADDY_API="http://localhost:2019"
    WEB_ROUTE_ID="virtue-web-${DOMAIN}"
    LANDING_ROUTE_ID="virtue-landing-${DOMAIN}"

    export VIRTUE_DEV_CA_CERT="$HOME/.local/share/caddy/pki/authorities/local/root.crt"
    export NODE_EXTRA_CA_CERTS="$HOME/.local/share/caddy/pki/authorities/local/root.crt"

    # Remove any stale routes from a previous run
    curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}-http" > /dev/null 2>&1 || true
    curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}-http" > /dev/null 2>&1 || true

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

    export VITE_API_URL="https://app.${DOMAIN}.localhost/api"
    export VITE_API_PROXY_TARGET="http://localhost:${API_PORT}"
    export VITE_LANDING_URL="https://${DOMAIN}.localhost"
    export __VITE_ADDITIONAL_SERVER_ALLOWED_HOSTS="app.${DOMAIN}.localhost,${DOMAIN}.localhost"
    export PUBLIC_APP_URL="https://app.${DOMAIN}.localhost"
    printf '\n  Landing : http://%s.localhost  /  https://%s.localhost\n' "$DOMAIN" "$DOMAIN"
    printf '  Web     : http://app.%s.localhost  /  https://app.%s.localhost\n' "$DOMAIN" "$DOMAIN"
    printf '  API     : https://app.%s.localhost/api\n\n' "$DOMAIN"

    cleanup() {
        trap '' INT TERM
        curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${WEB_ROUTE_ID}-http" > /dev/null 2>&1 || true
        curl -sf -X DELETE "${CADDY_API}/id/${LANDING_ROUTE_ID}-http" > /dev/null 2>&1 || true
        kill 0 2>/dev/null || true
        wait 2>/dev/null || true
    }

    run "api"     "31" "api"     bun run dev -- --port "$API_PORT" \
        --var "APP_URL:https://app.${DOMAIN}.localhost" \
        --var "R2_URL:https://app.${DOMAIN}.localhost/r2" \
        --var "HASH_SERVER_URL:http://localhost:${API_PORT}/api"
    run "web"     "32" "web"     bun run dev -- --port "$WEB_PORT" --host 127.0.0.1
    run "landing" "34" "landing" bun run dev -- --port "$LANDING_PORT" --host 127.0.0.1
else
    export VITE_API_URL="http://localhost:${API_PORT}"
    export VITE_API_PROXY_TARGET="http://localhost:${API_PORT}"
    export VITE_LANDING_URL="http://localhost:${LANDING_PORT}"
    printf '\n  Landing : http://localhost:%s\n' "$LANDING_PORT"
    printf '  Web     : http://localhost:%s\n' "$WEB_PORT"
    printf '  API     : http://localhost:%s\n\n' "$API_PORT"

    run "api"     "31" "api"     bun run dev -- --port "$API_PORT" \
        --var "APP_URL:http://localhost:${WEB_PORT}" \
        --var "R2_URL:http://localhost:${API_PORT}/r2" \
        --var "HASH_SERVER_URL:http://localhost:${API_PORT}/api"
    run "web"     "32" "web"     bun run dev -- --port "$WEB_PORT"
    run "landing" "34" "landing" bun run dev -- --port "$LANDING_PORT"
fi

wait
