#!/usr/bin/env bash
# Decrypts an encrypted screenshot archive into RAM, runs the nsfw_eval collector on it, and
# deletes the plaintext again. Only blurred images and classifier numbers reach OUT_DIR.
#
# Usage: collect.sh ARCHIVE.tar.gz.enc PASSPHRASE_FILE OUT_DIR [collector options...]
#
# The archive is `openssl enc -aes-256-cbc -pbkdf2 -iter 600000` over a .tar.gz of PNGs with
# labels (labels.jsonl and/or manifest.json); set NSFW_EVAL_ITER for a different iteration
# count. PNGs at the archive root are collected into OUT_DIR; otherwise each subdirectory
# holding PNGs (desktop/, mobile-android/, ...) is collected into OUT_DIR/<subdirectory>.
# Collector options, including --model, are listed at the top of
# client/core/examples/nsfw_eval/main.rs.
set -euo pipefail

if [ $# -lt 3 ]; then
  sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi
archive=$(realpath "$1")
passphrase=$(realpath "$2")
out=$(realpath -m "$3")
shift 3
repo=$(cd "$(dirname "$0")/../.." && pwd)

# /dev/shm is RAM-backed, so the decrypted screenshots never touch the disk.
tmp_root=/dev/shm
[ -d "$tmp_root" ] && [ -w "$tmp_root" ] || tmp_root=${TMPDIR:-/tmp}
work=$(mktemp -d "$tmp_root/nsfw-eval.XXXXXX")
trap 'rm -rf "$work"' EXIT

openssl enc -d -aes-256-cbc -pbkdf2 -iter "${NSFW_EVAL_ITER:-600000}" \
  -pass "file:$passphrase" -in "$archive" | tar -xz -C "$work"

sets=()
if compgen -G "$work/*.png" > /dev/null; then
  sets=(".")
else
  for d in "$work"/*/; do
    compgen -G "$d*.png" > /dev/null && sets+=("$(basename "$d")")
  done
fi
[ ${#sets[@]} -gt 0 ] || { echo "no PNG screenshots found in $archive" >&2; exit 1; }

cd "$repo/client"
cargo build --release -q -p virtue-core --example nsfw_eval
for set in "${sets[@]}"; do
  dest="$out"
  [ "$set" = "." ] || dest="$out/$set"
  mkdir -p "$dest"
  # Labels and URLs, not images; score.py reads them from here.
  for f in labels.jsonl manifest.json; do
    if [ -f "$work/$set/$f" ]; then cp "$work/$set/$f" "$dest/$f"; fi
  done
  cargo run --release -q -p virtue-core --example nsfw_eval -- --out "$dest" "$@" "$work/$set"
  # Load time, per-run time and memory for the same model, for score.py's cost estimates.
  cargo run --release -q -p virtue-core --example nsfw_eval -- --bench "$@" > "$dest/bench.json"
done
