#!/bin/sh

# Sets up the repo (installs deps, copies config files, etc.)
#
# Usage: setup.sh [--tmux|-t] [worktree]
#
#   worktree       optional path for a new git worktree to create and set up
#   --tmux|-t      requires worktree; instead of setting up in this shell,
#                  opens a new tmux window at the worktree and runs setup
#                  there, then splits it into a `claude` pane and a plain
#                  terminal pane

tmux_mode=0
worktree=""

while [ $# -gt 0 ]; do
  case "$1" in
    --tmux|-t)
      tmux_mode=1
      shift
      ;;
    *)
      worktree="$1"
      shift
      ;;
  esac
done

if [ "$tmux_mode" = "1" ] && [ -z "$worktree" ]; then
  echo "Error: --tmux/-t requires a worktree path, e.g. setup.sh --tmux ../my-worktree" >&2
  exit 1
fi

if [ "$tmux_mode" = "1" ] && [ -z "$TMUX" ]; then
  echo "Error: --tmux/-t must be run from inside a tmux session" >&2
  exit 1
fi

initial_dir="$(pwd)"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

if [ ! -z "$worktree" ]; then
  git fetch
  branch="$(basename "$worktree")"
  git branch "$branch" origin/staging
  git worktree add "$worktree" "$branch"
  ROOT="$ROOT/$worktree"
  echo "Creating new worktree: $worktree (branch $branch)"
fi

if [ "$tmux_mode" = "1" ]; then
  worktree_abs="$(cd "$worktree" && pwd)"
  session="$(tmux display-message -p '#S')"
  name="$(basename "$worktree_abs")"

  tmux new-window -t "$session" -n "$name" -c "$worktree_abs"
  tmux send-keys -t "$session:$name" './scripts/setup.sh && claude' Enter
  tmux split-window -h -t "$session:$name" -c "$worktree_abs"

  echo "Worktree created at $worktree_abs"
  echo "Opening tmux window '$name' (setup + claude in one pane, a plain terminal in the other)"
  exit 0
fi

echo "Repo root: $ROOT"

setdir() {
  cd "$ROOT/$1"
}

# just — install if missing (the root justfile is the command catalog: `just --list`).
# The justfile uses [group(...)] recipe attributes (just >= 1.27), which is newer
# than the version in most distro package managers, so install the prebuilt
# binary from the official install script rather than via apt.
if ! command -v just > /dev/null 2>&1; then
  echo "Installing just..."
  curl --proto '=https' --tlsv1.2 -sSf https://just.systems/install.sh \
    | sudo bash -s -- --to /usr/local/bin
fi

# Web deps
setdir "." && bun install

# Shared config that's identical across worktrees (secrets, machine-local
# paths), then .env (repo root, per-worktree overrides) on top of it. See
# AGENTS.md for the recognized keys.
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

# Copies keys already set in $VIRTUE_DEV_ENV onto a freshly copied .dev.vars,
# so a new worktree doesn't need them retyped. $1 = keys to check, space-separated.
apply_shared_env() {
  for key in $1; do
    eval "value=\${$key:-}"
    if [ -n "$value" ]; then
      # .dev.vars.example already ships a placeholder line for most of these
      # keys, so drop any existing line for this key before appending the
      # real value from the shared env.
      grep -v "^${key}=" .dev.vars > .dev.vars.tmp || true
      mv .dev.vars.tmp .dev.vars
      printf '%s=%s\n' "$key" "$value" >> .dev.vars
    fi
  done
}

# Web setup
setdir "api"
cp .dev.vars.example .dev.vars
apply_shared_env "JWT_PRIVATE_KEY JWT_PUBLIC_KEY APP_URL LANDING_URL APP_NAME API_BASE_PATH R2_URL HASH_SERVER_URL EMAIL_DELIVERY_MODE BUG_REPORT_EMAIL AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY AWS_REGION"
yes | bun run db:migrate:local
./scripts/seed-dev-user.mjs

# Donations API setup
setdir "api-donate"
cp .dev.vars.example .dev.vars
apply_shared_env "STRIPE_SECRET_KEY STRIPE_WEBHOOK_SECRET"
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

if [ ! -z "$worktree" ]; then
  cd "$worktree"
  echo 'Set up in worktree!'
fi

