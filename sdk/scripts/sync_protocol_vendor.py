#!/usr/bin/env python3
"""Vendor the node's protocol-types crate exactly and rewrite its manifest.

Copies every file of NODE_ROOT/crates/protocol-types, plus NODE_ROOT/LICENSE,
into vendor/dytallix-protocol-types, removes vendored files the node no
longer has, and records each file's size and SHA-256 in
vendor/protocol-types-source.json. check_protocol_vendor.py --node-root then
passes; CI runs it so the SDK cannot drift from the chain (E04 gap 8).
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

VENDOR = "vendor/dytallix-protocol-types"
CRATE = "crates/protocol-types"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--node-root", type=Path, required=True)
    args = parser.parse_args()
    sdk, node = args.sdk_root.resolve(), args.node_root.resolve()
    crate = node / CRATE
    if not (crate / "Cargo.toml").is_file() or not (node / "LICENSE").is_file():
        print("node root has no protocol-types crate or LICENSE", file=sys.stderr)
        return 1
    skipped = {"target", "__pycache__"}
    sources = {p.relative_to(crate).as_posix(): p for p in sorted(crate.rglob("*"))
               if p.is_file() and not skipped.intersection(p.relative_to(crate).parts)
               and p.suffix != ".pyc"}
    sources["LICENSE"] = node / "LICENSE"
    vendor = sdk / VENDOR
    for path in sorted(vendor.rglob("*"), reverse=True):
        relative = path.relative_to(vendor).as_posix()
        if path.is_file() and relative not in sources:
            path.unlink()
        elif path.is_dir() and not any(path.iterdir()):
            path.rmdir()
    records = []
    for relative, source in sorted(sources.items()):
        target = vendor / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        data = target.read_bytes()
        records.append({
            "path": relative,
            "source_path": "LICENSE" if relative == "LICENSE" else f"{CRATE}/{relative}",
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
    head = subprocess.run(["git", "-C", str(node), "rev-parse", "HEAD"], capture_output=True,
                          text=True, check=True).stdout.strip()
    manifest_path = sdk / "vendor/protocol-types-source.json"
    manifest = json.loads(manifest_path.read_text())
    manifest.update({
        "source_repository": "https://github.com/exocognosis/dytallix",
        "source_head": head,
        "source_kind": "working_tree_snapshot",
        "source_head_is_complete_snapshot": False,
        "source_description": ("Exact bytes of mainnet/node/crates/protocol-types and mainnet/node/LICENSE, "
                               "copied by scripts/sync_protocol_vendor.py. The file hashes, not "
                               "source_head alone, identify this snapshot."),
        "files": records,
    })
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"files": len(records), "source_head": head}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
