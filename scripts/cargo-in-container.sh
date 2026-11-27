#!/usr/bin/env bash
# Runs cargo inside the build env image, for machines without Servo's -devel packages.
# The resulting binary runs on the host as long as the runtime libs (gstreamer, mesa, ...) are there.
# Usage: scripts/cargo-in-container.sh build --release
set -euo pipefail

image="${DRIFT_BUILD_IMAGE:-ghcr.io/xrookiefight/drift-build-env:latest}"
engine="$(command -v podman || command -v docker)"
root="$(cd "$(dirname "$0")/.." && pwd)"
cache="${XDG_CACHE_HOME:-$HOME/.cache}/drift-build"
mkdir -p "$cache/registry" "$cache/git"

exec "$engine" run --rm -i \
  -v "$root:/work:Z" \
  -v "$cache/registry:/usr/local/cargo/registry:Z" \
  -v "$cache/git:/usr/local/cargo/git:Z" \
  -w /work \
  -e CARGO_TERM_COLOR=always \
  "$image" cargo "$@"
