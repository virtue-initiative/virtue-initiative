#!/bin/sh

# Sets up the repo (installs deps, copies config files, etc.)

initial_dir="$(pwd)"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

setdir() {
  cd "$ROOT/$1"
}

# Web deps
setdir "." && bun install

# Web setup
setdir "api"
cp .dev.vars.example .dev.vars
yes | bun run db:migrate:local

# Donations API setup
setdir "api-donate"
cp .dev.vars.example .dev.vars
yes | bun run db:migrate:local

# Caddy — install if missing
if ! command -v caddy > /dev/null 2>&1; then
  echo "Installing Caddy..."
  sudo apt-get install -y debian-keyring debian-archive-keyring apt-transport-https curl
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
    | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    | sudo tee /etc/apt/sources.list.d/caddy-stable.list
  sudo apt-get update && sudo apt-get install -y caddy
fi

# Allow Caddy to bind to port 443 as a regular user
sudo setcap 'cap_net_bind_service=+ep' "$(command -v caddy)"

# Trust Caddy's local CA (system store + NSS databases for browsers)
sudo caddy trust

# Also install Caddy CA certs into Firefox's own NSS cert store.
# caddy trust handles the system store but not Firefox's cert9.db on Linux,
# and security.enterprise_roots.enabled doesn't bridge that gap reliably.
if command -v certutil > /dev/null 2>&1; then
    for profile in $(find "$HOME/.mozilla/firefox/" -name "cert9.db" 2>/dev/null | xargs -I{} dirname {}); do
        for cert in /usr/local/share/ca-certificates/Caddy_*.crt; do
            certutil -A -n "$(basename "$cert" .crt)" -t "CT,," -i "$cert" -d "sql:$profile" > /dev/null 2>&1 || true
        done
    done
fi

# Start Caddy if not already running; prefer systemctl if a unit exists
if ! curl -sf http://localhost:2019/config/ > /dev/null 2>&1; then
    if systemctl cat caddy > /dev/null 2>&1; then
        sudo systemctl start caddy
    else
        caddy start --config "$ROOT/scripts/caddy-base.json" > /dev/null 2>&1
    fi
fi

# Ensure the dev server is configured (caddy-base.json); systemctl uses Caddyfile which lacks it
# Caddy API returns 200 with "null" when the path doesn't exist
if [ "$(curl -sf http://localhost:2019/config/apps/http/servers/dev 2>/dev/null)" = "null" ] || \
   [ "$(curl -sf http://localhost:2019/config/apps/http/servers/dev-http 2>/dev/null)" = "null" ]; then
    caddy reload --config "$ROOT/scripts/caddy-base.json" > /dev/null 2>&1
fi

cd "$initial_dir"
