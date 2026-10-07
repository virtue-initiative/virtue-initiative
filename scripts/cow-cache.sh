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
# The Windows VM gets the same treatment on a ReFS Dev Drive inside the VM
# (see client/windows/VM_SETUP.md); client/windows/scripts/remote-windows-build.sh
# picks it up automatically once a base exists:
#
#   warm-windows   (re)build the VM's base from origin/staging
#   prune-windows  delete VM build roots whose worktree no longer exists
#
# Env:
#   VIRTUE_COW_DIR   mount point (default /mnt/virtue-cow)
#   VIRTUE_COW_IMG   image file for `init` (default ~/storage/virtue-cow.img,
#                    or ~/virtue-cow.img if ~/storage doesn't exist)
#   VIRTUE_COW_SIZE  image size for `init` (default 80G; the file is sparse)
#   VIRTUE_WIN_HOST      SSH host of the Windows VM (default virtue-win11)
#   VIRTUE_WIN_COW_ROOT  cache root on the VM's Dev Drive (default V:/virtue)

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
# binaries, and the Android JNI library. virtue-linux needs leptonica/tesseract
# dev libs (see client/linux/README.md), so it's only warmed when they're
# installed.
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
                && CARGO_TARGET_DIR="$target" cargo test --no-run $pkgs) || return 1
            build_android_jni "$src" "$target"
            ;;
    esac
}

# The Android JNI library, built exactly as client/android/app/build.gradle.kts's
# buildRustNative task does (same NDK path, ABIs and --release), so a worktree's
# Gradle build finds it already compiled. Skipped if the Android toolchain
# isn't installed (see client/android/README.md).
build_android_jni() {
    local src="$1" target="$2"
    local sdk="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"
    local ndk_version
    ndk_version="$(sed -n 's/^ *ndkVersion = "\(.*\)"/\1/p' "$src/client/android/app/build.gradle.kts")"
    local ndk="${ANDROID_NDK_ROOT:-$sdk/ndk/$ndk_version}"
    if ! command -v cargo-ndk > /dev/null 2>&1 || [ ! -d "$ndk" ]; then
        echo "cow-cache: cargo-ndk or the Android NDK ($ndk) not found, skipping Android" >&2
        return 0
    fi
    (cd "$src/client/android" \
        && ANDROID_SDK_ROOT="$sdk" ANDROID_HOME="$sdk" ANDROID_NDK_ROOT="$ndk" ANDROID_NDK_HOME="$ndk" \
            CARGO_TARGET_DIR="$target" cargo ndk -t arm64-v8a -t x86_64 -o "$target/android-jnilibs" \
            build --release --locked --manifest-path rust/Cargo.toml)
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

WIN_HOST="${VIRTUE_WIN_HOST:-virtue-win11}"
WIN_COW="${VIRTUE_WIN_COW_ROOT:-V:/virtue}"

# Runs PowerShell from stdin on the VM. -EncodedCommand sidesteps the quoting
# of ssh -> cmd -> powershell entirely.
win_ps() {
    local script
    script="$(printf '$ProgressPreference = "SilentlyContinue"\n$ErrorActionPreference = "Stop"\n'; cat)"
    ssh -n -o BatchMode=yes "$WIN_HOST" "powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $(printf '%s' "$script" | iconv -f utf-8 -t utf-16le | base64 -w0)"
}

require_win_cow() {
    ssh -o BatchMode=yes -o ConnectTimeout=10 "$WIN_HOST" "echo ok" > /dev/null 2>&1 \
        || die "can't reach $WIN_HOST over SSH (is the VM running?)"
    local drive="${WIN_COW%%:*}"
    [ "$(win_ps <<< "Test-Path '${drive}:\\'" | tr -d '\r')" = "True" ] \
        || die "$WIN_HOST has no ${drive}: Dev Drive (see client/windows/VM_SETUP.md)"
}

cmd_warm_windows() {
    require_win_cow
    git -C "$ROOT" fetch origin staging
    local rev
    rev="$(git -C "$ROOT" rev-parse origin/staging)"
    local build="$ROOT/client/windows/scripts/remote-windows-build.sh"
    local next="$WIN_COW/base.new"

    win_ps <<< "if (Test-Path '$next') { Remove-Item -Recurse -Force '$next' }"
    # smoke builds the host-target crates plus clippy; msix builds the
    # x86_64-pc-windows-msvc target and the packaged app. Warm both.
    "$build" --build-host "$WIN_HOST" --no-cow --mode smoke \
        --build-root "$next" --target-dir "$next/cargo-target" --source-rev "$rev"
    "$build" --build-host "$WIN_HOST" --no-cow --mode msix --skip-sync \
        --build-root "$next" --target-dir "$next/cargo-target"

    win_ps <<EOF
Set-Content -Path '$next/info' -Value "rev=$rev"
if (Test-Path '$WIN_COW/base') { Remove-Item -Recurse -Force '$WIN_COW/base' }
Rename-Item -Path '$next' -NewName 'base'
EOF
    echo "cow-cache: Windows base warmed at ${rev:0:9}."
}

cmd_prune_windows() {
    require_win_cow
    local key src
    # Each build root records the worktree it was built from (see
    # remote-windows-build.sh); drop the ones whose worktree is gone.
    win_ps <<< "Get-ChildItem '$WIN_COW/worktrees' -Directory -ErrorAction SilentlyContinue | ForEach-Object { \"\$(\$_.Name)\`t\$(Get-Content (Join-Path \$_.FullName 'source') -ErrorAction SilentlyContinue)\" }" \
        | tr -d '\r' | while IFS="$(printf '\t')" read -r key src; do
            [ -n "$key" ] || continue
            if [ -z "$src" ] || [ ! -d "$src" ]; then
                echo "cow-cache: removing $WIN_COW/worktrees/$key (${src:-unknown worktree} is gone)"
                win_ps <<< "Remove-Item -Recurse -Force '$WIN_COW/worktrees/$key'"
            fi
        done
}

case "${1:-}" in
    init) cmd_init ;;
    warm) cmd_warm ;;
    link) cmd_link ;;
    prune) cmd_prune ;;
    status) cmd_status ;;
    warm-windows) cmd_warm_windows ;;
    prune-windows) cmd_prune_windows ;;
    *)
        sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
        exit 1
        ;;
esac
