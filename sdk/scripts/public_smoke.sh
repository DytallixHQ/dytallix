#!/usr/bin/env bash
# Local smoke checks. The public testnet checks left with the legacy client
# (E04 gap 19); the mainnet SDK has no default public endpoint.

set -euo pipefail

MODE="${1:-}"

case "$MODE" in
  first-keypair)
    cargo run --locked -p dytallix-sdk --example first-keypair
    ;;
  *)
    echo "usage: $0 first-keypair" >&2
    exit 1
    ;;
esac
