# Bridge And Benchmarks

[Docs hub](README.md) | [Algorithms](algorithms.md) | [FAQ](faq.md)

## Bridge Support

[`src/bridge.rs`](../src/bridge.rs) extends the base cryptography layer with:

- chain-aware payload models
- bridge signatures containing chain ID, timestamp, validator ID, nonce, and sequence
- replay-protection helpers
- multi-validator signature validation
- per-chain hashing and address-format configuration

Supported payload shapes currently include:

- Ethereum-style transactions
- Cosmos IBC packets
- generic bridge payloads

## Benchmarking

[`src/performance.rs`](../src/performance.rs) provides helpers for:

- key-generation timing
- signature timing
- verification timing
- signature and public-key size reporting
- estimated gas-cost analysis

The benchmark layer currently compares:

- ML-DSA-87
- FN-DSA-1024
- SLH-DSA-SHA2-128s

and derives a simple recommendation from the measured results.
