#!/usr/bin/env python3
"""Check docs/architecture/interfaces-v1.json against the source (E04 gap 9).

Every interface entry must name an existing file and symbol, a version
(a constant with its recorded value, a literal present in a file, or an
explicit "none" with the reason), and tests that exist. Paths are relative
to the repository's mainnet/ directory. The node's tests separately check
that the inventory's ABCI query paths are exactly the ones the application
accepts.
"""
import json
import re
import sys
from pathlib import Path

MAINNET = Path(__file__).resolve().parents[2]
INVENTORY = MAINNET / "node/docs/architecture/interfaces-v1.json"
GROUPS = {"abci_query", "transaction", "result", "engine_rpc", "p2p",
          "operator_output", "internal", "sdk", "cli", "file"}
EXPOSURES = {"external", "operator", "internal"}


def read(root, relative, cache):
    if relative not in cache:
        path = root / relative
        cache[relative] = path.read_text(encoding="utf-8") if path.is_file() else None
    return cache[relative]


def constant_holds(text, name, value):
    # Rust `const NAME: T = V;`, Go `NAME = V` in a const block or `const NAME = V`.
    pattern = rf"\b{re.escape(name)}\b\s*(?::\s*\w+)?\s*(?:\w+\s*)?=\s*{re.escape(str(value))}\b"
    return re.search(pattern, text) is not None


def test_exists(text, name):
    return re.search(rf"\b(?:fn|func)\s+{re.escape(name)}\b", text) is not None


def check(inventory, root=MAINNET):
    errors = []
    cache = {}
    if inventory.get("version") != 1:
        errors.append("inventory version must be 1")
    entries = inventory.get("interfaces", [])
    ids = [entry.get("id") for entry in entries]
    for duplicate in sorted({i for i in ids if ids.count(i) > 1}):
        errors.append(f"duplicate id {duplicate}")
    for entry in entries:
        name = entry.get("id", "<no id>")
        if entry.get("group") not in GROUPS:
            errors.append(f"{name}: unknown group {entry.get('group')!r}")
        if entry.get("exposure") not in EXPOSURES:
            errors.append(f"{name}: unknown exposure {entry.get('exposure')!r}")
        if not entry.get("format"):
            errors.append(f"{name}: format missing")
        source = entry.get("source", {})
        text = read(root, source.get("file", ""), cache)
        if text is None:
            errors.append(f"{name}: source file {source.get('file')} missing")
        elif source.get("symbol", "") not in text:
            errors.append(f"{name}: symbol {source.get('symbol')!r} not in {source.get('file')}")
        version = entry.get("version", {})
        if "none" in version:
            if not str(version["none"]).strip():
                errors.append(f"{name}: an unversioned interface needs its reason")
        elif "constant" in version:
            constant = version["constant"]
            text = read(root, constant["file"], cache)
            if text is None:
                errors.append(f"{name}: version file {constant['file']} missing")
            elif not constant_holds(text, constant["name"], constant["value"]):
                errors.append(f"{name}: {constant['name']} = {constant['value']} not in {constant['file']}")
        elif "literal" in version:
            literal = version["literal"]
            text = read(root, literal["file"], cache)
            if text is None or literal["text"] not in text:
                errors.append(f"{name}: version text {literal['text']!r} not in {literal['file']}")
        else:
            errors.append(f"{name}: version needs a constant, a literal or none")
        if entry.get("group") == "abci_query" and not (entry.get("path") and entry.get("variant")):
            errors.append(f"{name}: query entries need a path and a variant")
        for test in entry.get("tests", []):
            text = read(root, test["file"], cache)
            if text is None or not test_exists(text, test["name"]):
                errors.append(f"{name}: test {test['name']} not in {test['file']}")
    return errors


def main():
    inventory = json.loads(INVENTORY.read_text(encoding="utf-8"))
    errors = check(inventory)
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print(json.dumps({"status": "PASS", "interfaces": len(inventory["interfaces"])}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
