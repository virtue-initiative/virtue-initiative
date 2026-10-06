#!/usr/bin/env bash
# Shares Rust build output across worktrees with copy-on-write (reflink) copies.
#
# A base build of origin/staging lives on a reflink-capable filesystem (XFS or
# btrfs; on an ext4 machine, a loop-mounted XFS image). Each worktree's
# client/target and hash-server/target become symlinks to its own reflinked
# copy of that base, so a new worktree starts with every dependency already
# compiled and only rebuilds the workspace's own crates. The copy is instant
# and takes no extra disk until files diverge.
#
# Usage: cow-cache.sh <command>
#
#   init     one-time (sudo): create, format and mount an XFS image at
#            $VIRTUE_COW_DIR, with an fstab entry so it survives reboots
#   warm     (re)build the shared base from origin/staging; rerun after a Rust
#            toolchain upgrade or a big Cargo.lock change
#   link     give this worktree reflinked target dirs (setup.sh runs this)
#   prune    delete cached target dirs whose worktree no longer exists
#   status   show the mount, base build and linked worktrees
#
# Env:
#   VIRTUE_COW_DIR   mount point (default /mnt/virtue-cow)
#   VIRTUE_COW_IMG   image file for `init` (default ~/storage/virtue-cow.img,
#                    or ~/virtue-cow.img if ~/storage doesn't exist)
#   VIRTUE_COW_SIZE  image size for `init` (default 80G; the file is sparse)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COW="${VIRTUE_COW_DIR:-/mnt/virtue-cow}"
BASE="$COW/base"
TREES="$COW/worktrees"
LOCK="$COW/.lock"

# Cargo projects whose target dir is shared. `client` is the Rust workspace;
# hash-server is standalone.
COMPONENTS="client hash-server"

export PATH="$HOME/.cargo/bin:$PATH"

die() {
    echo "cow-cache: $*" >&2
    exit 1
}

require_mount() {
    mountpoint -q "$COW" || die "$COW is not mounted (run: $0 init)"
}

rustc_version() {
    rustc --version 2>/dev/null || echo unknown
}

cmd_init() {
    if mountpoint -q "$COW"; then
        echo "$COW is already mounted."
        return
    fi
    local default_img="$HOME/virtue-cow.img"
    [ -d "$HOME/storage" ] && default_img="$HOME/storage/virtue-cow.img"
    local img="${VIRTUE_COW_IMG:-$default_img}"
    local size="${VIRTUE_COW_SIZE:-80G}"

    command -v mkfs.xfs > /dev/null 2>&1 || [ -x /usr/sbin/mkfs.xfs ] \
        || sudo apt-get install -y xfsprogs
    if [ ! -f "$img" ]; then
        truncate -s "$size" "$img"
        /usr/sbin/mkfs.xfs -q "$img"
    fi
    sudo mkdir -p "$COW"
    # discard: deleting files on the image punches holes in the sparse file,
    # giving the space back to the host filesystem.
    grep -q " $COW " /etc/fstab \
        || echo "$img $COW xfs loop,discard,noatime 0 0" | sudo tee -a /etc/fstab > /dev/null
    sudo systemctl daemon-reload 2> /dev/null || true
    sudo mount "$COW"
    sudo chown "$(id -u):$(id -g)" "$COW"
    echo "Mounted $img at $COW."
}

# What every worktree is likely to build: launch.sh's `cargo run`, plus test
# binaries. virtue-linux needs leptonica/tesseract dev libs (see
# client/linux/README.md), so it's only warmed when they're installed.
build_component() {
    local name="$1" src="$2" target="$3"
    case "$name" in
        hash-server)
            (cd "$src/hash-server" && CARGO_TARGET_DIR="$target" cargo build \
                && CARGO_TARGET_DIR="$target" cargo test --no-run)
            ;;
        client)
            local pkgs="-p virtue-core"
            if pkg-config --exists lept tesseract 2> /dev/null; then
                pkgs="$pkgs -p virtue-linux"
            else
                echo "cow-cache: leptonica/tesseract dev libs not found, skipping virtue-linux" >&2
            fi
            # shellcheck disable=SC2086
            (cd "$src/client" && CARGO_TARGET_DIR="$target" cargo build $pkgs \
                && CARGO_TARGET_DIR="$target" cargo test --no-run $pkgs)
            ;;
    esac
}

cmd_warm() {
    require_mount
    exec 9> "$LOCK"
    flock 9

    git -C "$ROOT" fetch origin staging
    local rev
    rev="$(git -C "$ROOT" rev-parse origin/staging)"

    # Build from scratch into a new dir, then swap it in. Building on top of
    # the old base would keep every stale artifact from older dependency
    # versions forever. Worktrees' reflinked copies are independent files, so
    # deleting the old base doesn't affect them. Plain export rather than a
    # git worktree, so it doesn't show up in `git worktree list`.
    local next="$BASE.new"
    rm -rf "$next"
    mkdir -p "$next/src"
    git -C "$ROOT" archive "$rev" client hash-server | tar -x -C "$next/src"

    # A component that fails to build still keeps whatever dependencies did
    # compile, so worktrees get a partial head start rather than none.
    local failed=""
    for c in $COMPONENTS; do
        echo "cow-cache: building base $c..."
        build_component "$c" "$next/src" "$next/$c-target" || failed="$failed $c"
    done

    printf 'rev=%s\nrustc=%s\n' "$rev" "$(rustc_version)" > "$next/info"
    rm -rf "$BASE"
    mv "$next" "$BASE"
    if [ -n "$failed" ]; then
        echo "cow-cache: base warmed at ${rev:0:9}, but these failed to build:$failed" >&2
        exit 1
    fi
    echo "cow-cache: base warmed at ${rev:0:9}."
}

# Worktree basenames aren't unique (the main checkout is just "main"), so key
# each worktree's cache dir by basename plus a hash of its absolute path.
tree_key() {
    printf '%s-%s' "$(basename "$ROOT")" "$(printf '%s' "$ROOT" | sha256sum | cut -c1-8)"
}

cmd_link() {
    require_mount
    [ -f "$BASE/info" ] || die "no base build yet (run: $0 warm)"
    exec 9> "$LOCK"
    flock -s 9

    local base_rustc
    base_rustc="$(sed -n 's/^rustc=//p' "$BASE/info")"
    if [ "$base_rustc" != "$(rustc_version)" ]; then
        echo "cow-cache: base was built with $base_rustc, now $(rustc_version);" \
            "dependencies will rebuild until you rerun: $0 warm" >&2
    fi

    local dir="$TREES/$(tree_key)"
    mkdir -p "$dir"
    printf '%s\n' "$ROOT" > "$dir/source"

    for c in $COMPONENTS; do
        local link="$ROOT/$c/target" cached="$dir/$c-target"
        if [ -L "$link" ]; then
            echo "cow-cache: $c/target already linked"
            continue
        fi
        if [ -e "$link" ]; then
            echo "cow-cache: $c/target is a real directory, leaving it alone" \
                "(delete it and rerun to switch to the shared cache)" >&2
            continue
        fi
        if [ ! -d "$cached" ] && [ -d "$BASE/$c-target" ]; then
            cp -a --reflink=always "$BASE/$c-target" "$cached"
        fi
        mkdir -p "$cached"
        ln -s "$cached" "$link"
        echo "cow-cache: $c/target -> $cached"
    done
}

cmd_prune() {
    require_mount
    [ -d "$TREES" ] || return 0
    for dir in "$TREES"/*/; do
        dir="${dir%/}"
        local src
        src="$(cat "$dir/source" 2> /dev/null || true)"
        if [ -z "$src" ] || [ ! -d "$src" ]; then
            echo "cow-cache: removing $dir (${src:-unknown worktree} is gone)"
            rm -rf "$dir"
        fi
    done
}

cmd_status() {
    if ! mountpoint -q "$COW"; then
        echo "$COW: not mounted"
        return
    fi
    df -h "$COW" | tail -1
    if [ -f "$BASE/info" ]; then
        echo "base: $(tr '\n' ' ' < "$BASE/info")"
    else
        echo "base: not built"
    fi
    for dir in "$TREES"/*/; do
        [ -f "$dir/source" ] && echo "linked: $(cat "$dir/source")"
    done
    return 0
}

case "${1:-}" in
    init) cmd_init ;;
    warm) cmd_warm ;;
    link) cmd_link ;;
    prune) cmd_prune ;;
    status) cmd_status ;;
    *)
        sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
        exit 1
        ;;
esac
