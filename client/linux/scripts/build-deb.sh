#!/usr/bin/env bash
set -euo pipefail

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

source "${CLIENT_ROOT}/scripts/version.sh"

BASE_VERSION="$(virtue_base_version)"
BUILD_LABEL="$(virtue_build_label)"
ARCH="$(dpkg --print-architecture)"
PKG_NAME="virtue"

cargo build --release -p virtue-linux-client

PKG_DIR="target/debian/${PKG_NAME}_${BUILD_LABEL}_${ARCH}"
OUT_DEB="target/debian/${PKG_NAME}_${BUILD_LABEL}_${ARCH}.deb"

rm -rf "$PKG_DIR" "$OUT_DEB"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/lib/systemd/user"
mkdir -p "$PKG_DIR/usr/share/doc/virtue"

install -m 0755 target/release/virtue "$PKG_DIR/usr/bin/virtue"
install -m 0644 linux/packaging/systemd/virtue.service "$PKG_DIR/usr/lib/systemd/user/virtue.service"
install -m 0644 linux/README.md "$PKG_DIR/usr/share/doc/virtue/README.md"
install -m 0755 linux/packaging/debian/postinst "$PKG_DIR/DEBIAN/postinst"
install -m 0755 linux/packaging/debian/prerm "$PKG_DIR/DEBIAN/prerm"

cat > "$PKG_DIR/DEBIAN/control" <<CONTROL
Package: $PKG_NAME
Version: $BASE_VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Virtue Initiative <support@virtue.app>
Depends: systemd
Description: Virtue Linux monitoring client
 Virtue command line and background service for screenshot monitoring.
CONTROL

dpkg-deb --root-owner-group --build "$PKG_DIR" "$OUT_DEB"
echo "Built $OUT_DEB"
