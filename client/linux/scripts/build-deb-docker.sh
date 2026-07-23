#!/usr/bin/env bash
set -euo pipefail

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

IMAGE_TAG="virtue-linux-deb-builder"
CARGO_CACHE_DIR="$CLIENT_ROOT/.docker-cargo-home"
DOCKER_TARGET_DIR="$CLIENT_ROOT/target-docker"

docker build -t "$IMAGE_TAG" -f linux/docker/Dockerfile linux/docker

mkdir -p "$CARGO_CACHE_DIR" "$DOCKER_TARGET_DIR"

# target/ is bind-mounted from a directory separate from the host's normal
# build output (target-docker/, not target/). Build artifacts are tied to the
# glibc/rustc of whatever environment produced them; reusing the host's
# native target/ here (e.g. from a CI cache populated by host-run cargo
# test/clippy steps) fails with "version `GLIBC_2.xx' not found" errors when
# the container's older glibc tries to run binaries linked on the host.
docker run --rm \
    --user "$(id -u):$(id -g)" \
    -e CARGO_HOME=/workspace/.docker-cargo-home \
    -e HOME=/workspace/.docker-cargo-home \
    -v "$CLIENT_ROOT:/workspace" \
    -v "$DOCKER_TARGET_DIR:/workspace/target" \
    -w /workspace \
    "$IMAGE_TAG" \
    ./linux/scripts/build-deb.sh "$@"
