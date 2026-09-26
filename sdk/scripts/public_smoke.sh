#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-}"
CAPABILITIES_MANIFEST="$ROOT/docs/public-capabilities.json"

case "$MODE" in
  capabilities-contract)
    node_endpoint="${DYTALLIX_ENDPOINT:-http://localhost:3030}"
    capabilities_json="$(curl --fail --silent --show-error "${node_endpoint%/}/api/capabilities")"
    python3 - "$CAPABILITIES_MANIFEST" "$capabilities_json" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
payload = json.loads(sys.argv[2])

assert payload["canonicalStatement"] == manifest["canonicalStatement"], payload
assert payload["features"]["stakingWrites"] == "hidden", payload["features"]
assert payload["features"]["governanceWrites"] == "hidden", payload["features"]

direct_node_only = payload.get("publicNode", {}).get("directNodeOnlyRoutes", [])
assert "GET /v1/validators" in direct_node_only, direct_node_only
assert "GET /v1/delegations/:address" in direct_node_only, direct_node_only

print("Verified runtime capabilities contract:", json.dumps(payload["features"], sort_keys=True))
PY
    ;;
  capabilities-require-live)
    node_endpoint="${DYTALLIX_ENDPOINT:-http://localhost:3030}"
    output="$(DYTALLIX_ENDPOINT="$node_endpoint" CARGO_INCREMENTAL=0 cargo run --locked -p dytallix-cli -- chain capabilities --require-live)"
    printf '%s\n' "$output"
    printf '%s' "$output" | grep -q 'Source: live-node'
    ;;
  faucet-policy)
    faucet_status_json="$(curl --fail --silent --show-error https://dytallix.com/api/faucet/status)"
    python3 - "$CAPABILITIES_MANIFEST" "$faucet_status_json" <<'PY'
import json
import sys

manifest = json.load(open(sys.argv[1], encoding="utf-8"))
payload = json.loads(sys.argv[2])
faucet = manifest["faucet"]
limits_manifest = faucet["limits"]
status_response = faucet["statusResponse"]
limits = payload.get("limits", {})

for key in status_response["requiredKeys"]:
    assert key in payload, payload

assert payload.get("status") == status_response["statusValue"], payload
assert limits.get("dgt") == limits_manifest["dgt"], limits
assert limits.get("drt") == limits_manifest["drt"], limits
assert limits.get("cooldownMinutes") == limits_manifest["cooldownMinutes"], limits
assert limits.get("maxRequestsPerHour") == limits_manifest["maxRequestsPerHour"], limits

print("Verified public faucet policy:", json.dumps(limits, sort_keys=True))
PY
    ;;
  first-keypair)
    cargo run --locked -p dytallix-sdk --example first-keypair
    ;;
  first-transaction)
    cargo run --locked -p dytallix-sdk --features network --example first-transaction
    ;;
  contract-build)
    rustup target add wasm32-unknown-unknown
    cargo build --locked \
      --manifest-path "$ROOT/examples/contracts/minimal_contract/Cargo.toml" \
      --target wasm32-unknown-unknown \
      --release
    ;;
  *)
    echo "usage: $0 <capabilities-contract|capabilities-require-live|faucet-policy|first-keypair|first-transaction|contract-build>" >&2
    exit 1
    ;;
esac
