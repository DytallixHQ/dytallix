#!/usr/bin/env python3

from __future__ import annotations

import json
import pathlib
import sys
import urllib.request


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "docs" / "public-capabilities.json"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def fetch_json(url: str) -> dict:
    with urllib.request.urlopen(url, timeout=20) as response:
        return json.load(response)


def main() -> int:
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    live_status = fetch_json("https://dytallix.com/status")
    live_capabilities = fetch_json("https://dytallix.com/api/capabilities")
    live_validators = fetch_json("https://dytallix.com/api/blockchain/api/staking/validators")

    require(
        live_status.get("capabilities_endpoint") == "/api/capabilities",
        "Live status is missing the capabilities endpoint marker",
    )
    require(
        live_capabilities.get("canonicalStatement") == manifest.get("canonicalStatement"),
        "Live capabilities statement drifted from repo manifest",
    )
    require(
        live_capabilities.get("features", {}).get("stakingWrites") == "hidden",
        "Live stakingWrites feature drifted",
    )
    require(
        live_capabilities.get("features", {}).get("governanceWrites") == "hidden",
        "Live governanceWrites feature drifted",
    )

    validators = live_validators.get("validators", [])
    require(validators, "Live validator feed is empty")
    for validator in validators:
        address = validator.get("address", "")
        require(
            address.startswith("dytallix1"),
            f"Validator address is not D-Addr compatible: {address}",
        )
        require(
            address not in {"validator1", "validator2", "validator3", "validator4"},
            f"Placeholder validator leaked into live feed: {address}",
        )

    print("Verified live public deployment contract against repo manifest.")
    print(
        json.dumps(
            {
                "latest_height": live_status.get("latest_height"),
                "validator_count": live_validators.get("total_validators"),
                "capabilities_endpoint": live_status.get("capabilities_endpoint"),
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())