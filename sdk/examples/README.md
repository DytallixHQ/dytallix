# Examples

This example is the fastest way to check the SDK from the repository root.

## Available Examples

- [`first-keypair.rs`](first-keypair.rs) - generate an ML-DSA-65 keypair,
  derive a D-Addr, sign a message, and verify the signature

## Run

```bash
cargo run -p dytallix-sdk --example first-keypair
```

The example runs offline. For the consensus-chain CLI flow (wallet, pinned
chain, balance and send), see [Getting started](../docs/getting-started.md).

## Related Docs

- [Getting started](../docs/getting-started.md)
- [SDK reference](../docs/sdk-reference.md)
- [CLI reference](../docs/cli-reference.md)
