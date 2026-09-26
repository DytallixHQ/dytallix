#!/usr/bin/env python3
"""Build the test registry from locked source. Requires the wasm32 Rust target."""
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT
    ))
    subprocess.run([
        "cargo", "build", "--locked", "--release", "--target", "wasm32-unknown-unknown",
        "-p", "dytallix-registry-test-contract",
    ], cwd=ROOT, check=True)
    source = Path(metadata["target_directory"]) / "wasm32-unknown-unknown/release/dytallix_registry_test_contract.wasm"
    target = ROOT / "dytallix-fast-launch/node/tests/fixtures/registry.wasm"
    shutil.copyfile(source, target)
    print(f"Built registry fixture: {target}")


if __name__ == "__main__":
    main()
