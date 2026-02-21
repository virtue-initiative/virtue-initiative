#!/usr/bin/env bash
set -euo pipefail

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' linux/Cargo.toml | head -n1)"
ARCH="$(dpkg --print-architecture)"
PKG_NAME="bepure"

cargo build --release -p bepure-linux-client

PKG_DIR="target/debian/${PKG_NAME}_${VERSION}_${ARCH}"
OUT_DEB="target/debian/${PKG_NAME}_${VERSION}_${ARCH}.deb"

rm -rf "$PKG_DIR" "$OUT_DEB"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/lib/systemd/user"
mkdir -p "$PKG_DIR/usr/share/doc/bepure"

install -m 0755 target/release/bepure "$PKG_DIR/usr/bin/bepure"
install -m 0644 linux/packaging/systemd/bepure.service "$PKG_DIR/usr/lib/systemd/user/bepure.service"
install -m 0644 linux/README.md "$PKG_DIR/usr/share/doc/bepure/README.md"
install -m 0755 linux/packaging/debian/postinst "$PKG_DIR/DEBIAN/postinst"
install -m 0755 linux/packaging/debian/prerm "$PKG_DIR/DEBIAN/prerm"

cat > "$PKG_DIR/DEBIAN/control" <<CONTROL
Package: $PKG_NAME
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: BePure <support@bepure.app>
Depends: systemd
Description: BePure Linux monitoring client
 BePure command line and background service for screenshot monitoring.
CONTROL

dpkg-deb --root-owner-group --build "$PKG_DIR" "$OUT_DEB"
echo "Built $OUT_DEB"
