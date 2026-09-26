#!/usr/bin/env python3
"""Inspect the locked selected graph. This is not compiled-artifact approval."""
import argparse
import json
import re
import subprocess
from pathlib import Path

# Legacy network surfaces are excluded even when a dependency can use plain HTTP.
PROHIBITED = re.compile(
    r"^(?:ring|aws-lc-rs|aws-lc-sys|openssl(?:-sys|-src|-probe)?|native-tls|"
    r"rustls(?:-.*)?|tokio-rustls|hyper-rustls|hyper-tls|tokio-native-tls|"
    r"security-framework(?:-sys)?|ed25519(?:-dalek)?|curve25519-dalek|"
    r"x25519-dalek|rsa|p256|p384|p521|k256|ecdsa|elliptic-curve|"
    r"libp2p(?:-.*)?|reqwest|axum(?:-.*)?|hyper(?:-.*)?|warp|"
    r"tungstenite|tokio-tungstenite|wasmtime(?:-.*)?|dytallix-node|"
    r"dytallix-pqc|pqcrypto(?:-.*)?)$"
)
REQUIRED = {"dytallix-fast-node", "dytallix-storage", "dytallix-runtime-crypto",
            "dytallix-protocol-types", "dytallix-adaptive-emission",
            "dytallix-signature-policy", "fips204", "rocksdb"}

def inspect_tree(tree):
    packages = sorted({line.split()[0] for line in tree.splitlines() if line.strip()})
    fips_features = sorted({feature for line in tree.splitlines()
                            if line.startswith("fips204 ")
                            for feature in line.partition("|")[2].split(" ", 1)[0].split(",") if feature})
    unexpected_fips_features = sorted(set(fips_features) - {"default-rng", "ml-dsa-65"})
    prohibited = [name for name in packages if PROHIBITED.fullmatch(name)]
    missing = sorted(REQUIRED - set(packages))
    missing_fips_features = sorted({"default-rng", "ml-dsa-65"} - set(fips_features))
    return packages, prohibited, missing, fips_features, unexpected_fips_features, missing_fips_features


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    command = ["cargo", "tree", "--locked", "--offline", "-p", "dytallix-fast-node",
               "--no-default-features", "--features", "pqc-consensus",
               "-e", "normal,build", "--prefix", "none", "--format", "{p}|{f}"]
    if args.target:
        command += ["--target", args.target]
    result = subprocess.run(command, cwd=root, text=True, capture_output=True)
    if result.returncode:
        raise SystemExit(result.stderr)
    (packages, prohibited, missing, fips_features, unexpected_fips_features,
     missing_fips_features) = inspect_tree(result.stdout)
    report = {"status": "PASS" if not prohibited and not missing and not unexpected_fips_features and not missing_fips_features else "FAIL",
              "scope": "locked normal/build dependency graph; artifact and provider inspection still required",
              "command": command, "target": args.target or "host",
              "packages": packages, "package_count": len(packages),
              "prohibited_matches": prohibited, "missing_required": missing,
              "fips204_features": fips_features, "unexpected_fips204_features": unexpected_fips_features,
              "missing_fips204_features": missing_fips_features,
              "launch_approval": False}
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    args.output.with_suffix(".tree.txt").write_text(result.stdout)
    print(json.dumps({k: report[k] for k in ["status", "target", "package_count", "prohibited_matches", "missing_required", "unexpected_fips204_features", "launch_approval"]}))
    raise SystemExit(0 if report["status"] == "PASS" else 1)

if __name__ == "__main__":
    main()
