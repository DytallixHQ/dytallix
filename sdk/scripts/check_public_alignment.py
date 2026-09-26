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
    install_commands = manifest["install"]
    faucet_limits = manifest["faucet"]["limits"]
    direct_node_only = manifest["publicRoutes"]["directNodeOnly"]

    required_statement_files = [
        "README.md",
        "docs/README.md",
        "docs/getting-started.md",
        "docs/core-concepts.md",
        "docs/cli-reference.md",
        "docs/faq.md",
    ]
    for relative_path in required_statement_files:
        text = read_text(relative_path)
        require(
            statement in text,
            f"Missing canonical public statement in {relative_path}",
        )

    sdk_git_install = install_commands["sdk"][0]
    sdk_network_install = install_commands["sdk"][1]
    cli_git_install = install_commands["cli"][0]

    install_files = [
        "README.md",
        "docs/getting-started.md",
        "docs/sdk-reference.md",
        "docs/faq.md",
    ]
    install_blob = "\n".join(read_text(path) for path in install_files)
    require(sdk_git_install in install_blob, "Missing canonical SDK git install command")
    require(
        sdk_network_install in install_blob,
        "Missing canonical SDK network install command",
    )
    require(cli_git_install in install_blob, "Missing canonical CLI git install command")
    require(
        "cargo add dytallix-sdk\n" not in install_blob,
        "Found bare `cargo add dytallix-sdk` despite crates.io being unpublished",
    )

    faucet_policy_blob = "\n".join(
        read_text(path)
        for path in [
            "README.md",
            "docs/getting-started.md",
            "docs/core-concepts.md",
            "docs/cli-reference.md",
            "docs/faq.md",
        ]
    )
    require(
        f"`{faucet_limits['dgt']} DGT`" in faucet_policy_blob,
        "Public faucet DGT amount drifted from manifest",
    )
    require(
        f"`{faucet_limits['drt']} DRT`" in faucet_policy_blob,
        "Public faucet DRT amount drifted from manifest",
    )
    require(
        f"`{faucet_limits['cooldownSeconds']}` second cooldown" in faucet_policy_blob,
        "Public faucet cooldown drifted from manifest",
    )
    require(
        f"`{faucet_limits['maxRequestsPerHour']}` requests per hour" in faucet_policy_blob,
        "Public faucet hourly cap drifted from manifest",
    )

    cli_reference = read_text("docs/cli-reference.md")
    require(
        "disabled on the default public website gateway" in cli_reference,
        "CLI reference must state that public staking/governance writes are disabled",
    )

    require(
        "GET /api/capabilities" in direct_node_only,
        "SDK manifest must describe the direct-node capabilities endpoint",
    )
    core_concepts = read_text("docs/core-concepts.md")
    require(
        "GET /api/capabilities" in core_concepts,
        "Core concepts must mention the machine-readable capabilities endpoint",
    )
    cli_reference = read_text("docs/cli-reference.md")
    require(
        "the CLI consults `GET /api/capabilities`" in cli_reference,
        "CLI reference must mention capability-driven public write gating",
    )
    require(
        "dytallix chain capabilities" in cli_reference,
        "CLI reference must document the chain capabilities command",
    )
    require(
        "--require-live" in cli_reference,
        "CLI reference must document the require-live capabilities mode",
    )
    require(
        "Source:" in cli_reference,
        "CLI reference must explain how the capabilities command reports its source",
    )
    require(
        "capabilities-require-live" in cli_reference,
        "CLI reference must mention the require-live smoke path",
    )

    return 0


if __name__ == "__main__":
    sys.exit(main())