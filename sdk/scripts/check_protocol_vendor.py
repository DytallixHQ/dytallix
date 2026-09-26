#!/usr/bin/env python3
"""Verify the exact protocol snapshot without Cargo or a sibling checkout."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate manifest field: {key}")
        result[key] = value
    return result


def relative_path(value):
    if not isinstance(value, str):
        raise ValueError("manifest path must be a string")
    path = PurePosixPath(value)
    if path.is_absolute() or not path.parts or any(p in (".", "..") for p in path.parts) or path.as_posix() != value or "\\" in value:
        raise ValueError(f"invalid relative path: {value}")
    return path


def inspect_file(path, record):
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"missing regular source file: {path}")
    data = path.read_bytes()
    if len(data) != record["bytes"] or hashlib.sha256(data).hexdigest() != record["sha256"]:
        raise ValueError(f"source hash differs: {path}")


def verify(sdk_root, node_root=None):
    sdk_root = Path(sdk_root).resolve()
    manifest_path = sdk_root / "vendor/protocol-types-source.json"
    manifest = json.loads(manifest_path.read_text(), object_pairs_hook=unique_object)
    if manifest["schema_version"] != 1 or manifest["hash_algorithm"] != "sha256" or manifest["vendor_path"] != "vendor/dytallix-protocol-types":
        raise ValueError("unsupported protocol source manifest")
    if manifest["source_kind"] != "working_tree_snapshot" or manifest["source_head_is_complete_snapshot"] is not False:
        raise ValueError("source snapshot provenance must remain explicit")
    vendor = sdk_root / manifest["vendor_path"]
    if vendor.is_symlink() or not vendor.is_dir():
        raise ValueError("vendor must be a regular directory")
    expected = set()
    source_expected = set()
    for record in manifest["files"]:
        path = relative_path(record["path"])
        source_path = relative_path(record["source_path"])
        if str(path) in expected or str(source_path) in source_expected:
            raise ValueError("duplicate source manifest path")
        expected.add(str(path))
        source_expected.add(str(source_path))
        inspect_file(vendor / path, record)
        if node_root is not None:
            inspect_file(Path(node_root) / source_path, record)
    if not expected or "Cargo.toml" not in expected or "LICENSE" not in expected:
        raise ValueError("source manifest must include package metadata and license")
    actual = set()
    for path in vendor.rglob("*"):
        if path.is_symlink():
            raise ValueError(f"symlinks are not permitted in vendor: {path}")
        if path.is_file():
            actual.add(path.relative_to(vendor).as_posix())
    if actual != expected:
        raise ValueError(f"vendor inventory differs: extra={sorted(actual - expected)}, missing={sorted(expected - actual)}")
    if node_root is not None:
        crate = Path(node_root) / "crates/protocol-types"
        source_actual = {p.relative_to(Path(node_root)).as_posix() for p in crate.rglob("*") if p.is_file()}
        source_actual.add("LICENSE")
        if source_actual != source_expected:
            raise ValueError("canonical node source inventory differs from recorded snapshot")
    return {"status": "PASS", "files": len(expected), "bytes": sum(r["bytes"] for r in manifest["files"]), "canonical_node_compared": node_root is not None, "source_head_is_complete_snapshot": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--node-root", type=Path, help="Optional canonical node checkout; detect source drift as well as vendor tampering")
    args = parser.parse_args()
    try:
        print(json.dumps(verify(args.sdk_root, args.node_root), sort_keys=True))
        return 0
    except (ValueError, KeyError, TypeError, OSError) as exc:
        print(f"Protocol source check failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
