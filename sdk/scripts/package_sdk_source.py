#!/usr/bin/env python3
"""Create a deterministic SDK source archive; do not build or publish it."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile
import sys
sys.dont_write_bytecode = True
from check_protocol_vendor import verify

ROOT_FILES = (
    ".gitignore", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "LICENSE",
    "README.md", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md",
    "start-local.sh", "stop-local.sh",
)
ROOT_DIRS = ("crates", "vendor", "examples", "docs", "scripts", ".github")
EXCLUDED_DIRS = {"target", "__pycache__", ".git", "node_modules", "dist"}
EXCLUDED_FILES = {".DS_Store"}


def source_files(root):
    files = []
    for relative in ROOT_FILES:
        path = root / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"required source file missing or symlinked: {relative}")
        files.append(path)
    for directory in ROOT_DIRS:
        base = root / directory
        if base.is_symlink() or not base.is_dir():
            raise ValueError(f"required source directory missing or symlinked: {directory}")
        for path in base.rglob("*"):
            relative = path.relative_to(base)
            if any(part in EXCLUDED_DIRS for part in relative.parts) or path.name in EXCLUDED_FILES:
                continue
            if path.is_symlink():
                raise ValueError(f"source symlinks are not permitted: {path}")
            if path.is_file():
                files.append(path)
    return sorted(files, key=lambda p: p.relative_to(root).as_posix())


def package(root, output):
    root = root.resolve()
    output = output.resolve()
    if output == root or root in output.parents:
        raise ValueError("write the release archive outside the source tree")
    if output.exists():
        raise ValueError("output already exists; do not overwrite release artifacts")
    verify(root)
    inputs = []
    for path in source_files(root):
        data = path.read_bytes()
        mode = 0o755 if path.stat().st_mode & 0o111 else 0o644
        inputs.append((path.relative_to(root).as_posix(), data, mode))
    manifest = {
        "schema_version": 1, "artifact": "dytallix-sdk-source",
        "scope": "Standalone workspace source; registry dependencies are locked, not vendored. No build, publication, activation, or consensus proof is asserted.",
        "files": [{"path": name, "bytes": len(data), "mode": mode, "sha256": hashlib.sha256(data).hexdigest()} for name, data, mode in inputs],
    }
    data = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    inputs.append(("SOURCE-MANIFEST.json", data, 0o644))
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("xb") as raw:
        with gzip.GzipFile(filename="", fileobj=raw, mode="wb", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                for name, data, mode in inputs:
                    info = tarfile.TarInfo("dytallix-sdk/" + name)
                    info.size = len(data)
                    info.mode = mode
                    info.mtime = 0
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    archive.addfile(info, io.BytesIO(data))
    return {"archive": str(output), "sha256": hashlib.sha256(output.read_bytes()).hexdigest(), "bytes": output.stat().st_size, "source_files": len(manifest["files"])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(package(args.sdk_root, args.output), sort_keys=True))
        return 0
    except (ValueError, KeyError, TypeError, OSError, tarfile.TarError) as exc:
        print(f"Source packaging failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
