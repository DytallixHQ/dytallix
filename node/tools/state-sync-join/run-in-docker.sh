#!/bin/sh
# State sync join qualification (state sync v1, step C5) in a Linux
# container: builds the Linux binaries, then runs `state-sync-join` under
# no_new_privs and the production system call deny list (oci_seccomp.py).
# Diagnostic only; the E02 native job is the CI run.
#
#   tools/state-sync-join/run-in-docker.sh OUTPUT_DIR
#
# OUTPUT_DIR (new) receives the binaries, the seccomp profile and, after
# the run, the fixture work directory with every process log.
set -eu
node="$(cd "$(dirname "$0")/../.." && pwd)"
out="${1:?usage: run-in-docker.sh OUTPUT_DIR}"
mkdir "$out"
out="$(cd "$out" && pwd)"
mkdir "$out/bin"
arch="$(docker info --format '{{.Architecture}}')"
case "$arch" in aarch64|arm64) goarch=arm64 ;; x86_64|amd64) goarch=amd64 ;; *) echo "unsupported $arch" >&2; exit 2 ;; esac

(cd "$node/consensus/cometbft" &&
  GOOS=linux GOARCH="$goarch" CGO_ENABLED=0 go build -mod=readonly -trimpath -o "$out/bin/" \
    ./cmd/dytallix-pqc-engine ./cmd/dytallix-comet-bridge ./cmd/dytallix-light-export \
    ./cmd/dytallix-comet-fixture)
python3 "$node/tools/native-execution-policy/oci_seccomp.py" --output "$out/seccomp.json"

printf 'FROM rust:1.88-bookworm\nRUN apt-get update && apt-get install -y --no-install-recommends clang libclang-dev cmake pkg-config && rm -rf /var/lib/apt/lists/*\n' |
  docker build -q -t dyt-state-sync-join-build - >/dev/null
docker run --rm -v "$node:/src:ro" -v dyt-state-sync-join-cargo:/usr/local/cargo/registry \
  -v dyt-state-sync-join-target:/target -v "$out/bin:/out" \
  -e CARGO_TARGET_DIR=/target -e CARGO_INCREMENTAL=0 -e CARGO_PROFILE_DEV_DEBUG=0 -w /src \
  dyt-state-sync-join-build sh -c '
    cargo build --locked -p dytallix-fast-node --bin consensus_stdio &&
    cargo build --locked -p dytallix-native-supervisor --features qualification-fixtures --bin state-sync-join &&
    cp /target/debug/consensus_stdio /target/debug/state-sync-join /out/'

# The work directory stays inside the container: Unix sockets need a Linux
# file system. It is copied out afterwards, pass or fail.
name="dyt-state-sync-join-$$"
status=0
# --init: the harness owns its children and refuses to run as PID 1.
docker run --init --name "$name" --security-opt "seccomp=$out/seccomp.json" \
  --security-opt no-new-privileges -v "$out/bin:/c5/bin:ro" dyt-state-sync-join-build \
  /c5/bin/state-sync-join --bin /c5/bin --work /tmp/c5 || status=$?
docker cp "$name:/tmp/c5" "$out/work" >/dev/null 2>&1 || true
docker rm "$name" >/dev/null
exit "$status"
