#!/usr/bin/env python3
"""Refuse classical public-key cryptography and TLS in Cargo lockfiles.

P01, 28 September 2026: no classical public-key cryptography anywhere in the
stack (E04 gap 19). Cargo records packages that no enabled feature reaches,
so a lockfile without them is the strongest source-level statement; compiled
artifacts are inspected separately (T01).

Usage: check_no_classical.py [LOCKFILE ...]   (default: this SDK's Cargo.lock)
"""
import json
from pathlib import Path
import re
import sys

# Classical signatures and key exchange, TLS and X.509 stacks, QUIC.
PROHIBITED = re.compile(
    r"^(?:ring|aws-lc-rs|aws-lc-sys|boring(?:-sys)?|openssl(?:-sys|-src|-probe|-macros)?|"
    r"native-tls|tokio-native-tls|hyper-tls|schannel|security-framework(?:-sys)?|"
    r"rustls(?:-.*)?|tokio-rustls|hyper-rustls|webpki(?:-.*)?|x509(?:-.*)?|"
    r"ed25519(?:-.*)?|curve25519-dalek(?:-.*)?|x25519-dalek|ed448(?:-.*)?|x448|"
    r"rsa|dsa|p192|p224|p256|p384|p521|k256|sm2|ecdsa|elliptic-curve|"
    r"secp256k1(?:-.*)?|libsecp256k1(?:-.*)?|bls12_381(?:-.*)?|blst|"
    r"quinn(?:-.*)?|h3(?:-.*)?)$"
)
NAME = re.compile(r'^name = "([^"]+)"$', re.M)


def packages(text):
    """Package names of one lockfile, from its [[package]] entries."""
    names = []
    for entry in text.split("[[package]]")[1:]:
        match = NAME.search(entry)
        if match is None:
            raise ValueError("a lockfile package has no name")
        names.append(match.group(1))
    return sorted(set(names))


def check(paths):
    report = {"lockfiles": {}}
    for path in paths:
        names = packages(Path(path).read_text(encoding="utf-8"))
        if not names:
            raise ValueError(f"{path} lists no packages")
        report["lockfiles"][str(path)] = {
            "packages": len(names),
            "prohibited": [name for name in names if PROHIBITED.fullmatch(name)],
        }
    failed = any(entry["prohibited"] for entry in report["lockfiles"].values())
    report["status"] = "FAIL" if failed else "PASS"
    return report


def main(argv):
    paths = argv or [str(Path(__file__).resolve().parents[1] / "Cargo.lock")]
    try:
        report = check(paths)
    except (OSError, ValueError) as exc:
        print(f"Lockfile check failed: {exc}", file=sys.stderr)
        return 1
    print(json.dumps(report, sort_keys=True))
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
