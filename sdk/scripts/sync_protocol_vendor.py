#!/usr/bin/env python3
"""Vendor the node's shared crates exactly and rewrite their manifests.

For each crate in VENDORED (protocol-types, E04 gap 8; client-channel, E04
gap 19), copies every file of NODE_ROOT/<crate>, plus NODE_ROOT/LICENSE,
into its vendor directory, removes vendored files the node no longer has,
and records each file's size and SHA-256 in its manifest.
check_protocol_vendor.py --node-root then passes; CI runs it so the SDK
cannot drift from the chain.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

from check_protocol_vendor import VENDORED


def sync(sdk, node, spec):
    CRATE = spec["crate"]
    crate = node / CRATE
    if not (crate / "Cargo.toml").is_file() or not (node / "LICENSE").is_file():
        raise ValueError(f"node root has no {CRATE} crate or LICENSE")
    skipped = {"target", "__pycache__"}
    sources = {p.relative_to(crate).as_posix(): p for p in sorted(crate.rglob("*"))
               if p.is_file() and not skipped.intersection(p.relative_to(crate).parts)
               and p.suffix != ".pyc"}
    sources["LICENSE"] = node / "LICENSE"
    vendor = sdk / spec["vendor_path"]
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
    manifest_path = sdk / spec["manifest"]
    manifest = json.loads(manifest_path.read_text()) if manifest_path.is_file() else {
        "schema_version": 1,
        "package": spec["package"],
        "package_version": "0.1.0",
        "vendor_path": spec["vendor_path"],
        "hash_algorithm": "sha256",
    }
    manifest.update({
        "source_repository": "https://github.com/DytallixHQ/dytallix",
        "source_head": head,
        "source_kind": "working_tree_snapshot",
        "source_head_is_complete_snapshot": False,
        "source_description": (f"Exact bytes of node/{CRATE} and node/LICENSE, "
                               "copied by scripts/sync_protocol_vendor.py. The file hashes, not "
                               "source_head alone, identify this snapshot."),
        "files": records,
    })
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    return {"package": spec["package"], "files": len(records), "source_head": head}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--node-root", type=Path, required=True)
    args = parser.parse_args()
    sdk, node = args.sdk_root.resolve(), args.node_root.resolve()
    try:
        print(json.dumps([sync(sdk, node, spec) for spec in VENDORED]))
    except ValueError as exc:
        print(exc, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
