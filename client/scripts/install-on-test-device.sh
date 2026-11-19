#!/usr/bin/env bash
# Build this worktree's client and install it on a manual-test device. `vd <device> deploy` calls
# this. It lives in the repo, not next to vd, so it changes along with the build scripts it calls.
#
# All but mac also launch the app, and linux's package starts its service. That can only be done from inside the Mac's test account, which has
# no SSH access, so vd does it through the screen.
set -euo pipefail

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIN_HOST="${VD_WIN_SSH_HOST:-virtue-win11}"
LINUX_HOST="${VD_LINUX_SSH_HOST:-virtue-linux}"

device="${1:-}"
[[ $# -gt 0 ]] && shift

case "$device" in
  ios) exec "$CLIENT_ROOT/scripts/remote-apple-build.sh" --platform ios --run "$@" ;;
  mac) exec "$CLIENT_ROOT/scripts/remote-apple-build.sh" --platform mac "$@" ;;
  android) exec "$CLIENT_ROOT/android/scripts/install.sh" "$@" ;;
  win)
    build_output="$(mktemp)"
    trap 'rm -f "$build_output"' EXIT
    "$CLIENT_ROOT/windows/scripts/remote-windows-build.sh" --build-host "$WIN_HOST" --mode msix "$@" | tee "$build_output"
    msix="$(sed -n 's/^MSIX package built on VM at: //p' "$build_output" | tr -d '\r')"
    [[ -n "$msix" ]] || { echo "The build didn't report an MSIX path." >&2; exit 1; }
    ssh -o BatchMode=yes "$WIN_HOST" \
      "powershell -NoProfile -ExecutionPolicy Bypass -File '${msix%/dist/*}/scripts/install-from-ssh.ps1' -SetupDir '${msix%.msix}-setup'"
    echo "Installed $msix"
    ;;
  linux)
    # In Docker, so the package runs on the test VM's older glibc. The .deb's postinst starts the
    # service for the logged-in desktop user.
    "$CLIENT_ROOT/linux/scripts/build-deb-docker.sh" "$@"
    deb="$(ls -t "$CLIENT_ROOT"/target-docker/debian/*.deb | head -1)"
    scp -q -o BatchMode=yes "$deb" "$LINUX_HOST:/tmp/virtue.deb"
    ssh -o BatchMode=yes "$LINUX_HOST" \
      'sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --allow-downgrades /tmp/virtue.deb && rm -f /tmp/virtue.deb'
    echo "Installed $(basename "$deb")"
    ;;
  *)
    echo "Usage: $(basename "$0") <win|linux|android|mac|ios> [build script options]" >&2
    exit 1
    ;;
esac
