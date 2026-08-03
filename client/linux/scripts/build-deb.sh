#!/usr/bin/env bash
set -euo pipefail

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

source "${CLIENT_ROOT}/scripts/version.sh"

BASE_VERSION="$(virtue_base_version)"
BUILD_LABEL="$(virtue_build_label)"
ARCH="$(dpkg --print-architecture)"

INSTANCE=""
TYPE="release"
TYPEARG="--release"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --instance)
            INSTANCE="$2"
            shift 2
            ;;
        --debug)
            TYPE="debug"
            TYPEARG=""
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

PKG_NAME="${INSTANCE:+virtue-$INSTANCE}"
PKG_NAME="${PKG_NAME:-virtue}"
BIN_NAME="$PKG_NAME"

VIRTUE_BUILD_LABEL="$BUILD_LABEL" VIRTUE_INSTANCE="$INSTANCE" cargo build $TYPEARG -p virtue-linux


PKG_DIR="target/debian/${PKG_NAME}_${BUILD_LABEL}_${ARCH}"
OUT_DEB="target/debian/${PKG_NAME}-linux_${BUILD_LABEL}_${ARCH}.deb"

rm -rf "target/debian"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/lib/systemd/user"
mkdir -p "$PKG_DIR/usr/share/doc/$PKG_NAME"

install -m 0755 "target/$TYPE/virtue" "$PKG_DIR/usr/bin/$BIN_NAME"
install -m 0644 linux/README.md "$PKG_DIR/usr/share/doc/$PKG_NAME/README.md"

if [[ -n "$INSTANCE" ]]; then
    sed "s|exec /usr/bin/virtue daemon|exec /usr/bin/$BIN_NAME daemon|g" \
        linux/packaging/systemd/virtue.service \
        > "$PKG_DIR/usr/lib/systemd/user/${BIN_NAME}.service"
else
    install -m 0644 linux/packaging/systemd/virtue.service \
        "$PKG_DIR/usr/lib/systemd/user/virtue.service"
    install -m 0755 linux/packaging/debian/postinst "$PKG_DIR/DEBIAN/postinst"
    install -m 0755 linux/packaging/debian/prerm "$PKG_DIR/DEBIAN/prerm"
fi

# Bundle libtesseract/liblept/libjpeg into the package instead of depending on
# the OS-provided packages. Leptonica's shared-library package was renamed
# from liblept5 to libleptonica6 (SONAME bump) starting with Debian trixie /
# newer Ubuntu releases, so a .deb built against one naming fails to install
# on the other. libjpeg is worse: Debian dropped the libjpeg v8 ABI
# (SONAME libjpeg.so.8, package libjpeg8/libjpeg-turbo8) entirely in favor of
# the v6b-compatible libjpeg62-turbo (SONAME libjpeg.so.62) -- on a build
# machine that links leptonica against the v8 ABI (e.g. Ubuntu), there is no
# installable libjpeg8 package on Debian at all, not just a rename. Vendoring
# all three avoids depending on any of these OS package names.
# libjpeg is a NEEDED entry of liblept.so.5, not of the binary itself, so this
# has to walk the full transitive closure (ldd) rather than just the direct
# NEEDED list (readelf -d) of the top-level binary.
BUNDLE_SONAMES="$(ldd "$PKG_DIR/usr/bin/$BIN_NAME" | grep -oE 'lib(tesseract|lept|leptonica|jpeg)[^ ]*\.so\.[0-9]+' | sort -u)"

mkdir -p "$PKG_DIR/usr/lib/$PKG_NAME"
for soname in $BUNDLE_SONAMES; do
    resolved="$(ldd "$PKG_DIR/usr/bin/$BIN_NAME" | awk -v s="$soname" '$1==s {print $3}')"
    install -m 0644 "$resolved" "$PKG_DIR/usr/lib/$PKG_NAME/$soname"
done

# RPATH lets the dynamic linker find the bundled libs without needing them in
# the system ld cache. Each bundled .so also needs its own $ORIGIN RPATH
# because DT_RUNPATH does not propagate transitively to a library's own
# NEEDED entries (libtesseract.so.5 itself needs liblept.so.5).
patchelf --set-rpath "\$ORIGIN/../lib/$PKG_NAME" "$PKG_DIR/usr/bin/$BIN_NAME"
for soname in $BUNDLE_SONAMES; do
    patchelf --set-rpath '$ORIGIN' "$PKG_DIR/usr/lib/$PKG_NAME/$soname"
done

# Auto-detect remaining shared library dependencies via dpkg-shlibdeps, since
# the binary links libraries pulled in transitively by Cargo dependencies
# (leptess/tesseract-sys) that aren't tracked anywhere else. Run it across the
# main binary and the bundled libs together so leptonica/tesseract's own
# remaining sub-dependencies (libpng, libcurl, libtiff, ...) are still picked
# up, then strip out the bundled libs' own package entries since those are
# vendored, not system deps.
rm -rf debian
mkdir -p debian
cat > debian/control <<CONTROL
Source: $PKG_NAME
Section: utils
Priority: optional
Maintainer: Virtue Initiative <support@virtue.app>

Package: $PKG_NAME
Architecture: $ARCH
Depends: \${shlibs:Depends}
Description: Virtue Linux monitoring client
CONTROL
SHLIBS_DEPENDS="$(dpkg-shlibdeps -O "$PKG_DIR/usr/bin/$BIN_NAME" "$PKG_DIR"/usr/lib/"$PKG_NAME"/*.so.* 2>/dev/null \
    | sed -n 's/^shlibs:Depends=//p' \
    | sed -E 's/(^|, ?)(lib(tesseract5|lept5|leptonica6|jpeg8|jpeg-turbo8))( \([^)]*\))?//g' \
    | sed -E 's/^, //; s/, ,/,/g; s/, $//')"
rm -rf debian

cat > "$PKG_DIR/DEBIAN/control" <<CONTROL
Package: $PKG_NAME
Version: $BASE_VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Virtue Initiative <support@virtue.app>
Depends: systemd, $SHLIBS_DEPENDS
Description: Virtue Linux monitoring client
 Virtue command line and background service for screenshot monitoring.
CONTROL

dpkg-deb --root-owner-group --build "$PKG_DIR" "$OUT_DEB"
echo "Built $OUT_DEB"
