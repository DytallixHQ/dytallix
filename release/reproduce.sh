#!/usr/bin/env bash
# Build the release set in the pinned builder on this machine (E06), offline
# from the local crate and Go module caches, and optionally compare it with
# another build's SHA256SUMS:
#
#   release/reproduce.sh OUT_DIR [OTHER/SHA256SUMS]
#
# OUT_DIR must not exist. Build caches go to the Docker volume
# dytallix-release-work, which this script keeps; remove it yourself with
# `docker volume rm dytallix-release-work` when you no longer need it. On an
# arm64 host the x86_64 builder runs under emulation: slower, same bytes.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
mainnet="$(cd "$here/.." && pwd)"
out="${1:?usage: release/reproduce.sh OUT_DIR [OTHER/SHA256SUMS]}"
other="${2:-}"
if [ -e "$out" ]; then echo "$out must not exist" >&2; exit 2; fi
registry="${CARGO_HOME:-$HOME/.cargo}/registry"
modcache="$(go env GOMODCACHE)"
image="dytallix-release-builder:$(shasum -a 256 "$here/builder/Dockerfile" | cut -c1-12)"

docker build --platform linux/amd64 -t "$image" "$here/builder"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
# No network: every crate and module comes from the read-only local caches,
# checked against the lockfiles, as in CI.
docker run --rm --platform linux/amd64 --network none \
  -v "$mainnet:/src:ro" -v "$out:/out" -v dytallix-release-work:/work \
  -v "$registry:/host-cargo-registry:ro" -v "$modcache:/go/pkg/mod:ro" \
  "$image" sh -ec '
    mkdir -p /usr/local/cargo/registry
    cp -a /host-cargo-registry/cache /host-cargo-registry/index /usr/local/cargo/registry/
    python3 /src/release/build_release.py build --src /src --out /out --work /work --offline'

if [ -n "$other" ]; then
  python3 "$here/build_release.py" compare "$other" "$out/SHA256SUMS"
fi
