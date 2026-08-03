#!/usr/bin/env bash
set -euo pipefail

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

source "${CLIENT_ROOT}/scripts/version.sh"

# Resolve the git hash/build date/ref name on the host, where .git is
# present, and pass them through explicitly. The container only has client/
# bind-mounted (see below), not the repo root's .git dir, so version.sh's git
# lookups would otherwise silently resolve to an empty hash and a "detached"
# ref name inside the container -- the latter makes virtue_release_channel()
# resolve to "dev" even on a main build, producing a build label that doesn't
# match the one the calling workflow computed on the host.
VIRTUE_GIT_SHORT_HASH="$(virtue_git_short_hash)"
VIRTUE_BUILD_DATE="$(virtue_build_date)"
VIRTUE_GIT_REF_NAME="$(virtue_git_ref_name)"
export VIRTUE_GIT_SHORT_HASH VIRTUE_BUILD_DATE VIRTUE_GIT_REF_NAME

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
    -e VIRTUE_GIT_SHORT_HASH \
    -e VIRTUE_BUILD_DATE \
    -e VIRTUE_GIT_REF_NAME \
    -v "$CLIENT_ROOT:/workspace" \
    -v "$DOCKER_TARGET_DIR:/workspace/target" \
    -w /workspace \
    "$IMAGE_TAG" \
    ./linux/scripts/build-deb.sh "$@"
