# Examples

These examples are the fastest way to validate the current SDK and CLI flows
from the repository root.

## Available Examples

- [`first-keypair.rs`](first-keypair.rs) - generate an ML-DSA-65 keypair,
  derive a D-Addr, sign a message, and verify the signature
- [`first-transaction.rs`](first-transaction.rs) - request faucet funds for a
  sender wallet, transfer to a separate recipient, and wait for confirmation
  against the configured public testnet endpoints
- [`deploy-contract.rs`](deploy-contract.rs) - use the CLI keystore and prepare
  a first contract deployment flow
- [`contracts/minimal_contract`](contracts/minimal_contract) - build a minimal
  deployable WASM artifact for the public contract quickstart

## Run

```bash
cargo run -p dytallix-sdk --example first-keypair
cargo run -p dytallix-sdk --features network --example first-transaction
cargo run -p dytallix-cli --example deploy-contract
rustup target add wasm32-unknown-unknown
cargo build --manifest-path examples/contracts/minimal_contract/Cargo.toml --target wasm32-unknown-unknown --release
```

The minimal contract artifact builds locally as shown above. Deploys use
`POST /contracts/deploy` on the active CLI endpoint. The canonical public
endpoint `https://dytallix.com` supports this route; use a local or direct node
only when you explicitly want local testing or custom infrastructure.

## Related Docs

- [Getting started](../docs/getting-started.md)
- [SDK reference](../docs/sdk-reference.md)
- [CLI reference](../docs/cli-reference.md)
