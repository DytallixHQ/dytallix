#!/usr/bin/env python3
"""E01 cryptographic source inventory.

Maps every active asymmetric-cryptography route in the maintained source to
its files and their SHA-256 hashes. `--write` regenerates the committed
inventory; `--check` fails when a routed file changed without regenerating it,
so any change to a cryptographic path must update the reviewed record.

This is a source record. It grants no G35 or launch approval; compiled
artifacts and loaded providers are inspected in T01.
"""

import argparse
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "docs/mainnet/e01-crypto-inventory.json"

# Production routes run in the node, engine, bridge, root verifier or supervisor.
ROUTES = [
    {
        "role": "ordinary_transaction",
        "algorithm": "ML-DSA-65 (FIPS 204)",
        "policy": "Verifier accepts exact mldsa65; the wire codec reserves ML-DSA-87 (code 2), which every verifier rejects",
        "files": [
            "crates/runtime-crypto/src/ordinary.rs",
            "crates/protocol-types/src/ordinary.rs",
        ],
    },
    {
        "role": "recovery_transaction",
        "algorithm": "ML-DSA-65 (FIPS 204)",
        "policy": "Recovery and sponsor signatures; the wire codecs reserve ML-DSA-87, which the verifiers reject",
        "files": [
            "crates/runtime-crypto/src/recovery.rs",
            "crates/runtime-crypto/src/recovery_sponsor.rs",
            "crates/protocol-types/src/recovery_wire.rs",
            "crates/protocol-types/src/recovery_sponsor.rs",
        ],
    },
    {
        "role": "governance_v3_transaction",
        "algorithm": "ML-DSA-65 (FIPS 204)",
        "policy": "Mainnet network requires mldsa65; not yet executed by consensus",
        "files": ["crates/runtime-crypto/src/ordinary_v3.rs"],
    },
    {
        "role": "development_signing",
        "algorithm": "ML-DSA-65 (FIPS 204)",
        "policy": "Key generation and signing for fixtures, tests and qualification harnesses (state-sync-join); no production signing path uses it. The legacy signed-transaction route was removed (E04 gap 14)",
        "files": [
            "crates/runtime-crypto/src/lib.rs",
            "crates/runtime-crypto/src/dilithium_fips204.rs",
        ],
    },
    {
        "role": "account_address_derivation",
        "algorithm": "SHA3-256 over the origin public key (no signature)",
        "policy": "Origin codes name ML-DSA-65, ML-DSA-87 and legacy Dilithium5 for address derivation only; they select no permitted algorithm. Mainnet accounts require ML-DSA-65 exclusively",
        "files": [
            "crates/protocol-types/src/address.rs",
            "dytallix-fast-launch/node/src/ordinary_state.rs",
        ],
    },
    {
        "role": "validator_key_proof",
        "algorithm": "ML-DSA-65 (FIPS 204)",
        "policy": "Proof of possession for registered and rotated consensus keys; the operator tool generates keys and signs only proofs it builds for the engine genesis chain",
        "files": [
            "dytallix-fast-launch/node/src/ordinary_validator.rs",
            "dytallix-fast-launch/node/src/runtime/validator_lifecycle.rs",
            "crates/runtime-crypto/src/pqc_verify.rs",
            "consensus/cometbft/cmd/dytallix-validator-key/main.go",
        ],
    },
    {
        "role": "consensus_validator_signature",
        "algorithm": "ML-DSA-65 (Comet ml_dsa_65)",
        "policy": "Genesis and local validator keys must be ml_dsa_65; the validator key and state load from checked bytes; remote signing is not compiled into PQC-only builds",
        "files": [
            "consensus/cometbft/internal/enginepqc/engine.go",
            "consensus/cometbft/upstream/crypto/mldsa65/key.go",
            "consensus/cometbft/upstream/privval/file.go",
            "consensus/cometbft/upstream/node/privval_socket_pqc.go",
        ],
    },
    {
        "role": "peer_key_establishment_and_identity",
        "algorithm": "ML-KEM-768 + ML-DSA-65",
        "policy": "Authenticated pinned peers; no classical or plaintext fallback. The E03 negative-peer probe carries a copy of these files with an additive staging-only rejection observer; a change here requires refreshing that copy and its method review",
        "files": [
            "consensus/cometbft/internal/pqcp2p/establishment.go",
            "consensus/cometbft/internal/pqcp2p/connection.go",
            "consensus/cometbft/internal/pqcp2p/identity.go",
            "consensus/cometbft/internal/pqcp2p/seed_identity.go",
            "consensus/cometbft/internal/enginepqc/seed_key.go",
        ],
    },
    {
        "role": "client_channel",
        "algorithm": "ML-KEM-768 + ML-DSA-65",
        "policy": "Anonymous clients pin the endpoint's full ML-DSA-65 key; one fixed suite, no negotiation or plaintext fallback (docs/architecture/client-channel-v1.md). The adapter's channel listener signs with a role key of its own, from an owner-only seed; the supervisor probes it with the published pin. The SDK and CLI do not use it yet (gap 19 C-c)",
        "files": [
            "crates/client-channel/src/handshake.rs",
            "crates/client-channel/src/pin.rs",
            "consensus/pqc-http-adapter/src/channel.rs",
            "consensus/pqc-http-adapter/src/bin/dytallix-channel-key.rs",
            "crates/native-supervisor/src/channel_probe.rs",
        ],
    },
    {
        "role": "peer_admission_and_startup",
        "algorithm": "Full ML-DSA-65 peer-key pins",
        "policy": "No negotiated fallback; --production remains blocked; the candidate profile runs only on reserved staging chains",
        "files": [
            "consensus/cometbft/internal/enginepqc/engine.go",
            "consensus/cometbft/internal/enginepqc/production_candidate.go",
            "consensus/cometbft/internal/enginepqc/remote_address.go",
            "consensus/cometbft/cmd/dytallix-pqc-engine/main.go",
        ],
    },
    {
        "role": "state_sync_light_client",
        "algorithm": "ML-DSA-65 (Comet ml_dsa_65 commit verification)",
        "policy": "A joining node verifies operator-exported light blocks from the configured trusted height by sequential verification; the trust period stays below the evidence age; PQC-only builds refuse RPC light-client servers",
        "files": [
            "consensus/cometbft/internal/lightblocks/lightblocks.go",
            "consensus/cometbft/internal/lightblocks/stateprovider.go",
            "consensus/cometbft/upstream/light/verifier.go",
            "consensus/cometbft/upstream/config/statesync_sources_pqc.go",
            "consensus/cometbft/upstream/node/services_pqc.go",
        ],
    },
    {
        "role": "root_authorization",
        "algorithm": "SLH-DSA-SHAKE-256s",
        "policy": "Emergency, upgrade, handover and restart controls verify through the pinned local root-verify helper",
        "files": [
            "consensus/root-authorization/authorization.go",
            "consensus/root-authorization/key_validation.go",
            "dytallix-fast-launch/node/src/emergency_verifier.rs",
            "dytallix-fast-launch/node/src/upgrade/v1/upgrade.rs",
            "dytallix-fast-launch/node/src/release_handover.rs",
            "dytallix-fast-launch/node/src/release_handover/restart.rs",
        ],
    },
    {
        "role": "helper_startup_admission",
        "algorithm": "None (inherited owner channel, SHA-512 artifact check)",
        "policy": "No asymmetric helper signature",
        "files": [
            "consensus/owner-guard/protocol.go",
            "consensus/owner-guard/guard_linux.go",
        ],
    },
    {
        "role": "service_artifact_authorization",
        "algorithm": "None (source-pinned executable hashes)",
        "policy": "Owner process checks; the supervisor requires ML-DSA-65 validator key records",
        "files": [
            "crates/native-supervisor/src/processes.rs",
            "crates/native-supervisor/src/config.rs",
            "crates/release-runtime/src/ownership_security.rs",
        ],
    },
    {
        "role": "rpc_and_application_boundary",
        "algorithm": "None (local transport)",
        "policy": "Unix ABCI and IPC RPC under dytallix_pqc_ipc; HTTP/1 loopback adapter; no TLS",
        "files": [
            "consensus/cometbft/cmd/dytallix-comet-bridge/application.go",
            "consensus/cometbft/cmd/dytallix-comet-bridge/snapshots.go",
            "consensus/pqc-http-adapter/src/main.rs",
            "deploy/pqc-engine/supervise.py",
        ],
    },
]

# Development and fixture tools. They are not production binaries.
TOOLING = [
    "dytallix-fast-launch/node/src/bin/pqc_signer.rs",
    "consensus/cometbft/cmd/dytallix-comet-fixture/main.go",
    "consensus/cometbft/cmd/dytallix-comet-fixture/pqc_engine_fixture.go",
    "consensus/cometbft/cmd/dytallix-comet-proof/main.go",
    "consensus/cometbft/cmd/dytallix-comet-verify/main.go",
    "consensus/cometbft/cmd/dytallix-comet-verify/dynamic.go",
    "consensus/cometbft/cmd/dytallix-pqc-peer-probe/main.go",
]

# Checks that enforce the boundary in CI (see .github/workflows/mainnet.yml).
ENFORCEMENT = [
    {"check": "scripts/check_consensus_cargo_profile.py",
     "scope": "Locked Rust graph of dytallix-fast-node (pqc-consensus): no classical, TLS, HTTP or legacy PQC crates; FIPS 204 limited to ML-DSA-65"},
    {"check": "scripts/check_consensus_go_graph.py",
     "scope": "PQC-only Go graphs of the engine, bridge and root verifier: no prohibited packages; the classical fork packages removed from the fork (E04 gap 20); no remote-signer sources"},
    {"check": "go vet / go test / go build -tags dytallix_pqc_only,dytallix_pqc_ipc",
     "scope": "The production Go build and its tests"},
    {"check": "scripts/e01_inventory.py --check",
     "scope": "Routed files match this inventory"},
]

EXCEPTIONS = [
    "The selected Go graphs contain the standard library's crypto/internal/boring and crypto/internal/boring/sig. With CGO disabled these are disabled stubs and no-op markers; classifying them in the compiled executable belongs to T01.",
    "Package and symbol rules cannot detect every renamed or unknown algorithm. A clean inventory alone does not establish PQC compliance.",
    "The upstream Comet fork retains its classical packages for the default development build. Under dytallix_pqc_only they have no buildable files; they are not deleted.",
    "The production startup flag remains fail-closed. The candidate transport runs only with an explicit staging flag and a reserved staging chain identity.",
    "The ML-KEM-768 + ML-DSA-65 peer protocol still requires independent protocol review (P02).",
    "Production chain identity, addresses, keys, custody, the release commit, the full distribution inventory and independent review are not frozen (E05, E06, P01, P02).",
]

REQUIRED_PRODUCTION_CONFIGURATION = [
    {"path": "config/config.toml", "required": "Explicit P2P IP endpoint, exact persistent peers, loopback RPC, Unix ABCI, no remote signer or TLS"},
    {"path": "config/genesis.json", "required": "Approved chain ID and ML-DSA-65 validator set"},
    {"path": "application configuration (consensus_stdio --config)", "required": "Recovery and ordinary profiles, the only user-transaction paths (E04 gap 14); lifecycle as ordinary requires"},
    {"path": "config/pqc_transport.json", "required": "Candidate profile, exact chain ID, local full public key, full peer-key and endpoint pins, bounded handshake timeout"},
    {"path": "config/pqc_peer_seed.bin", "required": "Owner-only ML-DSA-65 peer seed, separate from the validator key"},
    {"path": "config/priv_validator_key.json", "required": "Approved distinct ML-DSA-65 validator identity"},
    {"path": "data/priv_validator_state.json", "required": "Persisted validator signing state"},
    {"path": "/etc/dytallix-pqc/<instance>.json", "required": "Service manifest with pinned executable and configuration hashes"},
    {"path": "native service configuration (dytallix-native-supervisor)", "required": "Metrics directory and interval for the engine and application files (E04 gap 15); block history mode; snapshots optional"},
]


def sha256(relative):
    return hashlib.sha256((ROOT / relative).read_bytes()).hexdigest()


def build():
    routes = [{**route, "files": [{"path": path, "sha256": sha256(path)} for path in route["files"]]}
              for route in ROUTES]
    return {
        "schema": "dytallix-e01-crypto-source-inventory-v2",
        "status": "SOURCE_ROUTES_MAPPED_WITH_OPEN_PRODUCTION_BOUNDARY",
        "source_root": "mainnet/node",
        "routes": routes,
        "tooling": [{"path": path, "sha256": sha256(path)} for path in TOOLING],
        "enforcement": ENFORCEMENT,
        "source_exceptions_and_limits": EXCEPTIONS,
        "required_production_configuration": [
            {**entry, "production_value_status": "UNSET"} for entry in REQUIRED_PRODUCTION_CONFIGURATION],
        "g35_accepted": False,
        "production_activation_authorized": False,
    }


def render(inventory):
    return json.dumps(inventory, indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = render(build())
    if args.write:
        INVENTORY.parent.mkdir(parents=True, exist_ok=True)
        INVENTORY.write_text(expected)
        return
    current = INVENTORY.read_text() if INVENTORY.exists() else ""
    if current != expected:
        committed = json.loads(current) if current else {"routes": [], "tooling": []}
        old = {f["path"]: f["sha256"] for r in committed["routes"] for f in r["files"]}
        old.update({f["path"]: f["sha256"] for f in committed.get("tooling", [])})
        new = build()
        fresh = {f["path"]: f["sha256"] for r in new["routes"] for f in r["files"]}
        fresh.update({f["path"]: f["sha256"] for f in new["tooling"]})
        changed = sorted(path for path in fresh if old.get(path) != fresh[path])
        print("E01 inventory is stale; review the change and run scripts/e01_inventory.py --write",
              file=sys.stderr)
        for path in changed:
            print(f"  changed: {path}", file=sys.stderr)
        raise SystemExit(1)


if __name__ == "__main__":
    main()
