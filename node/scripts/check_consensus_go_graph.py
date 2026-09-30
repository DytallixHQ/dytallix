#!/usr/bin/env python3
"""G35 source screen for the PQC-only Go consensus build.

Fails when a selected production package graph imports a prohibited package,
when a removed classical fork package returns to the fork (E04 gap 20), when
remote-signer sources return to the privval package, or when a fork source
file gains a Dytallix build constraint (the fork has one build). This is a
source-graph check; compiled-artifact and provider inspection remain T01.
"""

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess

COMMANDS = (
    ("consensus/cometbft", "./cmd/dytallix-pqc-engine"),
    ("consensus/cometbft", "./cmd/dytallix-comet-bridge"),
    # Operator tools on the validator host (E04 gap 17).
    ("consensus/cometbft", "./cmd/dytallix-validator-key"),
    ("consensus/cometbft", "./cmd/dytallix-operator-rpc"),
    ("consensus/root-authorization", "./cmd/dytallix-root-verify"),
)
# The fork has one build (E04 gap 20): no source file may select itself by a
# Dytallix build tag, which would let classical code return behind a tag.
DYTALLIX_CONSTRAINT = re.compile(r"^//go:build .*\bdytallix_", re.M)
# Fork packages that implemented classical cryptography, classical transport
# or gRPC. E04 gap 20 deleted them; each must stay absent from the fork, so an
# import fails to resolve instead of reaching a binary.
REMOVED_PACKAGES = (
    "github.com/cometbft/cometbft/crypto/ed25519",
    "github.com/cometbft/cometbft/crypto/secp256k1",
    "github.com/cometbft/cometbft/crypto/secp256k1eth",
    "github.com/cometbft/cometbft/crypto/bls12381",
    "github.com/cometbft/cometbft/lp2p",
    # The gRPC broadcast API and its generated service (test hygiene).
    "github.com/cometbft/cometbft/rpc/grpc",
    "github.com/cometbft/cometbft/proto/tendermint/rpc/grpc",
)
# Remote signing is not compiled into the fork.
PRIVVAL = "github.com/cometbft/cometbft/privval"
REMOTE_SIGNER_PREFIXES = ("signer_", "socket_", "retry_signer", "noise_", "msgs")


def parse_stream(raw):
    decoder = json.JSONDecoder()
    offset = 0
    while offset < len(raw):
        while offset < len(raw) and raw[offset].isspace():
            offset += 1
        if offset == len(raw):
            break
        value, offset = decoder.raw_decode(raw, offset)
        yield value


def load_rules(module):
    checker = module / "tools/pqc_boundary/check_boundary.py"
    spec = importlib.util.spec_from_file_location("pqc_boundary_rules", checker)
    rules = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(rules)
    return checker, rules


def evaluate_graph(rules, packages):
    """Prohibited and provider-review matches for one `go list -deps` result."""
    incomplete = sorted(p["ImportPath"] for p in packages if p.get("Error") or p.get("Incomplete"))
    prohibited = {p["ImportPath"]: rules.classify_package(p["ImportPath"])
                  for p in packages if rules.classify_package(p["ImportPath"])}
    providers = {p["ImportPath"]: rules.classify_provider_package(p["ImportPath"])
                 for p in packages if rules.classify_provider_package(p["ImportPath"])}
    return incomplete, prohibited, providers


def remote_signer_files(go_files):
    return sorted(name for name in go_files if name.startswith(REMOTE_SIGNER_PREFIXES))


def tagged_sources(module):
    return sorted(str(path.relative_to(module)) for path in module.rglob("*.go")
                  if DYTALLIX_CONSTRAINT.search(path.read_text(encoding="utf-8", errors="replace")))


def go(arguments, cwd, env, check=True):
    return subprocess.run(["go", *arguments], cwd=cwd, env=env, capture_output=True, text=True, check=check)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    module = root / "consensus/cometbft"
    checker, rules = load_rules(module)
    env = os.environ.copy()
    env.update(GOWORK="off", GOFLAGS="-mod=readonly", CGO_ENABLED="0", GOOS="linux", GOARCH="amd64")
    errors = []
    graphs = {}
    for module_rel, command in COMMANDS:
        arguments = ["list", "-deps", "-json", command]
        packages = list(parse_stream(go(arguments, root / module_rel, env).stdout))
        incomplete, prohibited, providers = evaluate_graph(rules, packages)
        name = f"{module_rel}:{command}"
        if incomplete:
            errors.append(f"{name}: incomplete packages {incomplete}")
        if prohibited:
            errors.append(f"{name}: prohibited packages {sorted(prohibited)}")
        graphs[name] = {
            "package_count": len(packages),
            "prohibited_package_matches": prohibited,
            "provider_containers_requiring_t01_review": providers,
            "package_import_paths": sorted(p["ImportPath"] for p in packages),
        }
    removed = {}
    fork = root / "consensus/cometbft/upstream"
    for package in REMOVED_PACKAGES:
        directory = fork / package.removeprefix("github.com/cometbft/cometbft/")
        result = go(["list", package], module, env, check=False)
        removed[package] = not directory.exists() and result.returncode != 0
        if not removed[package]:
            errors.append(f"{package} is present in the fork")
    privval_files = go(["list", "-f", "{{join .GoFiles \" \"}}", PRIVVAL], module, env).stdout.split()
    if remote_signer_files(privval_files):
        errors.append(f"remote-signer sources in privval: {remote_signer_files(privval_files)}")
    constrained = tagged_sources(module)
    if constrained:
        errors.append(f"Dytallix build constraints in fork sources: {constrained}")
    report = {
        "schema": "dytallix-consensus-go-graph-v1",
        "status": "FAIL" if errors else "PASS",
        "scope": "selected Go package graphs, removed classical packages and untagged fork sources; no binary, provider, host or protocol acceptance",
        "target": "linux/amd64",
        "cgo_enabled": False,
        "rule_source": str(checker.relative_to(root)),
        "rule_source_sha256": hashlib.sha256(checker.read_bytes()).hexdigest(),
        "graphs": graphs,
        "removed_from_fork": removed,
        "pqc_privval_files": privval_files,
        "dytallix_constrained_sources": constrained,
        "errors": errors,
        "launch_approval": False,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
    print(json.dumps({"status": report["status"], "errors": errors,
                      "package_counts": {name: graph["package_count"] for name, graph in graphs.items()}},
                     sort_keys=True))
    raise SystemExit(1 if errors else 0)


if __name__ == "__main__":
    main()
