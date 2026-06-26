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

# Stop any running Caddy so the restarted process picks up the new capability
caddy stop 2>/dev/null || true

# Start Caddy with the dev base config
caddy start --config "$ROOT/scripts/caddy-base.json"

cd "$initial_dir"
