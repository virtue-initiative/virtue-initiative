#!/usr/bin/env bash
# Make the Linux host's dev stack (scripts/launch.sh <domain>) reachable from
# the Windows VM under the same *.localhost names the host uses.
#
# Windows resolves every name ending in .localhost to loopback inside
# getaddrinfo, ahead of the hosts file, so hosts entries do not work: ping
# follows them, but curl, browsers and .NET do not. What does work is
# forwarding the VM's own loopback ports to the host, which is what this sets
# up. Caddy routes on the Host header, so ports 80 and 443 cover every dev
# domain at once and this only has to be run again if the VM is rebuilt.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

BUILD_HOST="virtue-win11"
HOST_IP=""
CA_CERT="$HOME/.local/share/caddy/pki/authorities/local/root.crt"

usage() {
  cat <<'EOF'
Usage: setup-vm-dev-network.sh [options]

Options:
  --build-host <ssh-host>   SSH host/alias for the Windows VM. Default: virtue-win11
  --host-ip <ip>            Host address as seen from the VM. Default: the
                            libvirt bridge address of the VM's own default route
  --ca-cert <path>          Caddy local root CA to trust in the VM.
                            Default: ~/.local/share/caddy/pki/authorities/local/root.crt
  -h, --help                Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build-host) BUILD_HOST="${2:-}"; shift 2 ;;
    --host-ip)    HOST_IP="${2:-}";    shift 2 ;;
    --ca-cert)    CA_CERT="${2:-}";    shift 2 ;;
    -h|--help)    usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

if [[ -z "$HOST_IP" ]]; then
  # The VM's default gateway is this host on the libvirt bridge.
  HOST_IP="$(ssh "$BUILD_HOST" \
    'powershell -NoProfile -Command "(Get-NetRoute -DestinationPrefix 0.0.0.0/0 | Sort-Object RouteMetric | Select-Object -First 1).NextHop"' \
    2>/dev/null | tr -d '\r\n ')"
fi

if [[ -z "$HOST_IP" ]]; then
  echo "Could not determine the host IP from the VM; pass --host-ip." >&2
  exit 1
fi

echo "VM:        $BUILD_HOST"
echo "Host IP:   $HOST_IP"

ssh "$BUILD_HOST" "powershell -NoProfile -ExecutionPolicy Bypass -Command -" <<PS
\$ErrorActionPreference = 'Stop'

# portproxy needs the IP Helper service.
Set-Service -Name iphlpsvc -StartupType Automatic
Start-Service -Name iphlpsvc -ErrorAction SilentlyContinue

foreach (\$port in 80, 443) {
  netsh interface portproxy delete v4tov4 listenaddress=127.0.0.1 listenport=\$port | Out-Null
  netsh interface portproxy add v4tov4 \`
    listenaddress=127.0.0.1 listenport=\$port \`
    connectaddress=$HOST_IP connectport=\$port | Out-Null
}

netsh interface portproxy show v4tov4
PS

if [[ -f "$CA_CERT" ]]; then
  # Only needed for https:// in a browser inside the VM; the Rust client can
  # use the http:// URL and needs no trust store changes.
  remote_cert='C:/Windows/Temp/caddy-root.crt'
  scp -q "$CA_CERT" "${BUILD_HOST}:${remote_cert}"
  ssh "$BUILD_HOST" \
    "powershell -NoProfile -Command \"Import-Certificate -FilePath 'C:\\Windows\\Temp\\caddy-root.crt' -CertStoreLocation Cert:\\LocalMachine\\Root | Out-Null\""
  echo "Trusted:   $(basename "$CA_CERT") in LocalMachine\\Root"
else
  echo "Skipped CA trust: $CA_CERT not found (https in the VM will not validate)."
fi

cat <<'EOF'

Done. From inside the VM, with `scripts/launch.sh <domain>` running on the host:

  http://app.<domain>.localhost          web app
  http://app.<domain>.localhost/api      API
  http://<domain>.localhost              landing

Build the Windows client against it (the URL is compile-time):

  just windows-build-ssh --mode msix --api-url http://app.<domain>.localhost/api
EOF
