# FAQ

[Docs hub](README.md) | [Getting started](getting-started.md) | [Algorithms](algorithms.md)

## Why is this a separate repo from `dytallix-node`?

So the cryptographic implementation can be audited, tested, and evolved
independently from the full node workspace.

## Which primitives are included right now?

Signature support:

- ML-DSA-65
- ML-DSA-87
- FN-DSA-1024
- SLH-DSA-SHA2-128s

Key exchange:

- ML-KEM-1024

## Is this only a library?

No. The repo ships both the Rust library and CLI helpers for key generation,
signing, verification, and evidence generation.

## Does this repo include bridge-specific cryptography?

Yes. See [`src/bridge.rs`](../src/bridge.rs) and
[Bridge and benchmarking notes](bridge-and-benchmarks.md).
