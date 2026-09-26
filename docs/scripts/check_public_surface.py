#!/usr/bin/env python3

from __future__ import annotations

import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CONFIG_PATH = ROOT / "public-surface.json"
DOCS_DIR = ROOT / "docs"
SCAN_PATHS = [ROOT / "README.md", *sorted(DOCS_DIR.glob("*.md"))]


def load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def fail(message: str, failures: list[str]) -> None:
    failures.append(message)


def main() -> int:
    config = load_json(CONFIG_PATH)
    failures: list[str] = []

    retired_hosts = config.get("retired_hosts", [])
    banned_strings = config.get("banned_strings", [])
    required_strings = config.get("required_strings", {})
    sdk_git = config["sdk"]["git_url"]

    for path in SCAN_PATHS:
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()

        for retired in retired_hosts:
            if retired in text:
                fail(f"{path.relative_to(ROOT)} contains retired host `{retired}`.", failures)

        for banned in banned_strings:
            if banned in text:
                fail(f"{path.relative_to(ROOT)} contains banned string `{banned}`.", failures)

        for line_number, line in enumerate(lines, start=1):
            if "cargo add dytallix-sdk" in line and f"--git {sdk_git}" not in line:
                fail(
                    f"{path.relative_to(ROOT)}:{line_number} uses `cargo add dytallix-sdk` without the canonical Git source.",
                    failures,
                )

    for relative_path, expected_strings in required_strings.items():
        path = ROOT / relative_path
        if not path.exists():
            fail(f"Required file `{relative_path}` is missing.", failures)
            continue
        text = path.read_text(encoding="utf-8")
        for expected in expected_strings:
            if expected not in text:
                fail(
                    f"{relative_path} is missing required string `{expected}`.",
                    failures,
                )

    if failures:
        print("Public surface check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print(
        f"Public surface check passed for {len(SCAN_PATHS)} Markdown files using {CONFIG_PATH.name}."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
