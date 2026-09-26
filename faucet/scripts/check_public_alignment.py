#!/usr/bin/env python3

from __future__ import annotations

import json
import pathlib
import ssl
import sys
import urllib.request

try:
    import certifi
except ImportError:  # pragma: no cover - best effort local TLS fix
    certifi = None


ROOT = pathlib.Path(__file__).resolve().parents[1]
MANIFEST_PATH = ROOT / "public-capabilities.json"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def open_json(url: str) -> dict:
    context = None
    if certifi is not None:
        context = ssl.create_default_context(cafile=certifi.where())
    with urllib.request.urlopen(url, timeout=20, context=context) as response:
        return json.load(response)


def main() -> int:
    manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    statement = manifest["canonicalStatement"]
    faucet = manifest["faucet"]
    readme = (ROOT / "README.md").read_text(encoding="utf-8")

    require(statement in readme, "Missing canonical statement in README.md")
    require(str(MANIFEST_PATH.name) in readme or "deploy/nginx/faucet-compat.conf" in readme, "README.md must reference the local manifest or compatibility config")
    require("canonical public faucet backend source" in readme, "README.md must classify the repo as the canonical faucet backend source")
    require("`10 DGT`" in readme, "README DGT limit drifted from manifest")
    require("`100 DRT`" in readme, "README DRT limit drifted from manifest")
    require("`60` second cooldown" in readme, "README cooldown drifted from manifest")
    require("`20` requests per hour" in readme, "README hourly cap drifted from manifest")

    limits = faucet["limits"]
    status_payload = open_json(faucet["statusUrl"])
    for key in faucet["statusResponse"]["requiredKeys"]:
        require(key in status_payload, f"Live faucet status missing key: {key}")

    live_limits = status_payload.get("limits", {})
    require(status_payload.get("status") == faucet["statusResponse"]["statusValue"], "Live faucet status value drifted")
    require(live_limits.get("dgt") == limits["dgt"], "Live faucet DGT limit drifted")
    require(live_limits.get("drt") == limits["drt"], "Live faucet DRT limit drifted")
    require(live_limits.get("cooldownMinutes") == limits["cooldownMinutes"], "Live faucet cooldown drifted")
    require(live_limits.get("maxRequestsPerHour") == limits["maxRequestsPerHour"], "Live faucet hourly cap drifted")

    check_url = faucet["checkUrlTemplate"].replace("{address}", "dytallix1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqh29xqk")
    check_payload = open_json(check_url)
    for key in faucet["checkResponse"]["requiredKeys"]:
        require(key in check_payload, f"Live faucet check response missing key: {key}")

    return 0


if __name__ == "__main__":
    sys.exit(main())