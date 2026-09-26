#!/usr/bin/env python3
"""Inspect one unstripped consensus application. This does not accept G35."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


PATTERNS = {
    "classical_asymmetric": r"(?i)(ed25519|curve25519|x25519|secp256|p256|p384|p521|ecdsa|ecdh|\brsa[_:]|ring_core|openssl|boringssl)",
    "legacy_pqc_implementation": r"(?i)(PQCLEAN_|pqcrypto_|pqcrystals|dilithium[235]::|falcon1024::|kyber[0-9]+::|sphincssha)",
    "unselected_mldsa_implementation": r"fips204(?:::|\.\.)ml_dsa_(44|87)",
    "prohibited_provider": r"(?i)(libssl|libcrypto|Security\.framework)",
}


def run(command):
    return subprocess.run(command, check=True, text=True, capture_output=True).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", required=True, type=Path)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    artifact = args.artifact.resolve(strict=True)
    with artifact.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if digest != args.expected_sha256:
        raise ValueError("Artifact differs from the expected build hash")
    description = run(["file", str(artifact)]).strip()
    if "Mach-O" in description:
        commands = [["nm", "-U", str(artifact)], ["otool", "-L", str(artifact)]]
        demangler = ["c++filt", "-_"]
    elif "ELF" in description:
        commands = [["nm", "--defined-only", str(artifact)], ["readelf", "-d", str(artifact)]]
        demangler = ["c++filt"]
    else:
        raise ValueError("Unsupported executable format")
    raw = run(commands[0])
    symbols = subprocess.run(demangler, input=raw, text=True, capture_output=True, check=True).stdout
    providers = run(commands[1])
    matches = {
        name: [line for line in (providers if name == "prohibited_provider" else symbols).splitlines()
               if re.search(pattern, line)]
        for name, pattern in PATTERNS.items()
    }
    required = {
        "ml_dsa_65": bool(re.search(r"fips204(?:::|\.\.)ml_dsa_65", symbols)),
        "snappy": bool(re.search(r"snappy::", symbols)),
        "lz4": bool(re.search(r"LZ4_(?:compress|decompress)", symbols)),
    }
    passed = not any(matches.values()) and all(required.values())
    # Keep raw outputs so reviewers can inspect false positives and rule limits.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for suffix, content in [(".nm.txt", raw), (".symbols.txt", symbols), (".providers.txt", providers)]:
        args.output.with_suffix(suffix).write_text(content)
    result = {
        "status": "PASS" if passed else "FAIL",
        "scope": "Bounded named-symbol and direct-library inspection of one unstripped application; not exhaustive absence proof",
        "artifact": str(artifact), "sha256": digest, "bytes": artifact.stat().st_size,
        "description": description, "commands": commands, "demangler": demangler,
        "patterns": PATTERNS, "matches": matches, "required_implementation_markers": required,
        "production_approval": False, "g35_accepted": False,
    }
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"status": result["status"], "matches": {k: len(v) for k, v in matches.items()},
                      "required_implementation_markers": required, "g35_accepted": False}))
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
