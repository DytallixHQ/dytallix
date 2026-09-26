# Algorithms

[Docs hub](README.md) | [Getting started](getting-started.md) | [Bridge and benchmarking notes](bridge-and-benchmarks.md)

## Signature Algorithms

The public documentation uses NIST's current naming. The implementation in
[`SignatureAlgorithm`](../src/lib.rs) still uses legacy enum names derived from
the upstream crates.

| Internal enum | Public designation | Standard status |
|---|---|---|
| `Dilithium3` | `ML-DSA-65` | FIPS 204 |
| `Dilithium5` | `ML-DSA-87` | FIPS 204 |
| `Falcon1024` | `FN-DSA-1024` | current NIST FIPS 206 naming |
| `SphincsSha256128s` | `SLH-DSA-SHA2-128s` | FIPS 205 |

## Key Exchange

The key-exchange layer follows the same pattern:

| Internal enum | Public designation | Standard status |
|---|---|---|
| `Kyber1024` | `ML-KEM-1024` | FIPS 203 |

via [`KeyExchangeAlgorithm`](../src/lib.rs).

## Current Module Map

- [`src/lib.rs`](../src/lib.rs) - core algorithms, keypairs, signing, verify,
  key persistence, and KEM support
- [`src/bridge.rs`](../src/bridge.rs) - bridge payload formats, replay
  protection, chain-specific hashing, and multi-signature validation
- [`src/performance.rs`](../src/performance.rs) - benchmark and gas-estimation
  helpers

## Supporting Primitives

The crate also uses:

- BLAKE3
- SHA-2
- SHA-3 / Keccak
- AES-GCM
- PBKDF2

These are used for hashing, payload normalization, and key-storage helpers.
