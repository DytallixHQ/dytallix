#!/usr/bin/env python3

from __future__ import annotations

import json
import pathlib
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "docs" / "public-capabilities.json"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def read_text(relative_path: str) -> str:
    return (ROOT / relative_path).read_text(encoding="utf-8")


def main() -> int:
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    statement = manifest["canonicalStatement"]
    public_node = manifest["publicNode"]
    contract_endpoint = public_node["contractEndpoint"]["path"]

    for relative_path in [
        "README.md",
        "docs/README.md",
        "docs/rpc-and-apis.md",
        "docs/faq.md",
    ]:
        require(statement in read_text(relative_path), f"Missing canonical statement in {relative_path}")

    rpc_text = read_text("docs/rpc-and-apis.md")
    require(
        "Stake and governance write routes are intentionally not public-complete" in rpc_text,
        "RPC docs must state that staking/governance writes are not public-complete",
    )
    require(
        contract_endpoint in rpc_text,
        "RPC docs must mention the machine-readable contract endpoint",
    )

    source_blob = "\n".join(
        [
            read_text("dytallix-fast-launch/node/src/main.rs"),
            read_text("dytallix-fast-launch/node/src/rpc/mod.rs"),
            read_text("dytallix-fast-launch/node/src/mempool/mod.rs"),
        ]
    )
    require("/api/capabilities" in source_blob, "Node source must expose the capabilities endpoint")
    for route in public_node.get("directNodeOnlyRoutes", []):
        require(route.startswith("GET /v1/"), f"Unexpected direct-node-only route shape: {route}")
    for prefix in public_node["reservedUnsupportedPayloadPrefixes"]:
        require(prefix in source_blob, f"Reserved unsupported payload prefix missing from node source: {prefix}")

    require(
        '"validator1"' not in read_text("dytallix-fast-launch/node/src/rpc/mod.rs"),
        "Public validator RPC still contains placeholder validator IDs",
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())