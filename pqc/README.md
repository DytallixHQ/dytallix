# Dytallix PQC

Post-quantum cryptography primitives and CLI tools for Dytallix.

This repository contains the standalone Rust cryptography crate extracted from
the Dytallix node workspace so the cryptographic implementation can be reviewed,
tested, and versioned independently.

## Quick Links

- [Docs hub](docs/README.md)
- [Getting started](docs/getting-started.md)
- [Algorithms](docs/algorithms.md)
- [CLI reference](docs/cli-reference.md)
- [Bridge and benchmarking notes](docs/bridge-and-benchmarks.md)
- [FAQ](docs/faq.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [License](LICENSE)

## What This Repository Contains

- [`src/lib.rs`](src/lib.rs) - key generation, signing, verification, KEM support,
  key persistence, and error types
- [`src/bridge.rs`](src/bridge.rs) - bridge-specific signing, payload handling,
  replay protection, and multi-signature validation
- [`src/performance.rs`](src/performance.rs) - PQC performance and gas-cost benchmarking
- [`src/bin/`](src/bin) - CLI helpers for key generation, raw key export,
  signing, verification, and PQC evidence generation

## Supported Primitives

Public-facing documentation in this repo uses the current NIST FIPS-era names.
The internal Rust enums still use legacy upstream identifiers for compatibility.

### Signatures

- ML-DSA-65 (`Dilithium3` lineage)
- ML-DSA-87 (`Dilithium5` lineage)
- FN-DSA-1024 (`Falcon1024` lineage)
- SLH-DSA-SHA2-128s (`SPHINCS+ SHA2-128s-simple` lineage)

### Key Exchange

- ML-KEM-1024 (`Kyber1024` lineage)

### Supporting Cryptography

- BLAKE3
- SHA-2
- SHA-3 / Keccak
- AES-GCM
- PBKDF2

See [Algorithms](docs/algorithms.md) for the implementation map and the
legacy-to-FIPS naming table.

## Prerequisites

Install [Rust](https://www.rust-lang.org/tools/install) with `rustup`. That
provides the Rust toolchain and `cargo` required to build the crate and run the
CLI tools in this repository.

## Build

```bash
cargo build --release
```

Run tests:

```bash
cargo test
```

## CLI Tools

The crate includes several helper binaries:

- `keygen`
- `keygen_raw`
- `sign`
- `verify`
- `pqc_evidence`

See [CLI reference](docs/cli-reference.md) for commands and file outputs.

## Why This Exists

Dytallix uses post-quantum primitives throughout the stack. Publishing the PQC
crate separately makes it easier to:

- audit the cryptographic implementation in isolation
- validate supported algorithms and key formats
- run standalone evidence generation and interoperability checks
- evolve cryptographic code without coupling every change to the full node repo

## Related Repositories

- [dytallix-sdk](https://github.com/DytallixHQ/dytallix-sdk)
- [dytallix-node](https://github.com/DytallixHQ/dytallix-node)
- [DytallixHQ](https://github.com/DytallixHQ)

## Evidence limits

`pqc_evidence` requires `--features pqc-real`. A missing feature or unexpected
verification result returns a nonzero exit status. It preserves the evidence
filenames and JSON result fields. It checks the PQCManager backend only.
A successful run is not FIPS conformance evidence.

The optional Dilithium metadata check is explicitly ignored by default. It
checks a declared hash, not known-answer vectors. Run it only with an explicit
reviewed `DYT_DILITHIUM3_META` fixture. Known-answer vector execution remains
an open release requirement.

## Authoritative source

The node workspace at DytallixHQ/dytallix-node, directory pqc-crypto, is the authoritative implementation. This repository distributes that component. Maintain package metadata here. Generate src/ and tests/ from the reviewed node component with scripts/export_component.py. Do not maintain a second implementation by hand.

Private key files are plaintext and require restricted storage. The loader validates both key pairs and refuses incomplete existing files. It does not generate a replacement identity after a load error. Files from the earlier serializer can lack secret keys; restore original key material through the operator procedure. Public KeyPair JSON continues to omit secret material.

Bridge signer registration and nonce state remain in memory. Restart persistence, validator changes, custody, and mainnet bridge approval remain separate work. Generated signers stay in the local manager until its shared signer store is dropped. The multi-signature API counts distinct validators and commits nonce state only when the batch reaches its threshold.
