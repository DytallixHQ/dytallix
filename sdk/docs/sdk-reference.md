# SDK Reference

[Docs hub](README.md) | [Getting started](getting-started.md) | [Examples](../examples/README.md)

## Crates

| Crate | Purpose | Link |
| --- | --- | --- |
| `dytallix-core` | PQC keypairs, addresses, signatures, hashing, and errors | [`crates/dytallix-core`](../crates/dytallix-core) |
| `dytallix-sdk` | Re-exported core types, transactions, keystore, optional HTTP clients | [`crates/dytallix-sdk`](../crates/dytallix-sdk) |
| `dytallix-cli` | End-user command-line workflows built on the SDK | [`crates/dytallix-cli`](../crates/dytallix-cli) |

## Install

Add the SDK from Git:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
```

Add the SDK with the network feature:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
```

## Feature Flags

| Feature | Default | Effect |
| --- | --- | --- |
| `network` | No | Enables the async node client and faucet client via `reqwest` |

Without `network`, the SDK still supports offline key generation, address
derivation, signing, verification, and keystore operations.

## Re-Exported Core Types

The root SDK crate re-exports the most common identity primitives:

- `DAddr`
- `DytallixKeypair`
- `KeyScheme`
- `verify_mldsa65`

These come from [`dytallix-core`](../crates/dytallix-core/src/lib.rs).

## Core SDK Types

| Type | Purpose | Source |
| --- | --- | --- |
| `Token` | Canonical DGT and DRT token enum | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `Balance` | DGT and DRT balances for one account | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `AccountState` | Address, pubkey hash, balance, nonce, and scheme | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `FeeEstimate` | Compute and bandwidth gas split, reported in micro-denominated network fees | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `TransactionReceipt` | Submitted transaction status and charged fee | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `Block`, `ChainStatus`, `Validator`, `Delegation` | Read models for node responses | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `FaucetStatus` | Faucet eligibility and retry window | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `KeystoreEntry` | Serialized key material and metadata | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |

## Modules

| Module | Purpose | Source |
| --- | --- | --- |
| `transaction` | Build, sign, hash, and estimate fees for transactions | [`transaction.rs`](../crates/dytallix-sdk/src/transaction.rs) |
| `keystore` | File-backed keystore open, save, list, and active-wallet logic | [`keystore.rs`](../crates/dytallix-sdk/src/keystore.rs) |
| `client` | Async node client for account, balance, block, status, validator, and delegation queries | [`client.rs`](../crates/dytallix-sdk/src/client.rs) |
| `faucet` | Async faucet client for funding and eligibility checks | [`faucet.rs`](../crates/dytallix-sdk/src/faucet.rs) |
| `error` | SDK error variants for cryptography, serialization, keystore, and network failures | [`error.rs`](../crates/dytallix-sdk/src/error.rs) |

The `client` and `faucet` modules are only compiled when the `network` feature
is enabled.

## Common Flows

### Offline Keypair and Address

```rust
use dytallix_sdk::{DAddr, DytallixKeypair};

let keypair = DytallixKeypair::generate();
let address = DAddr::from_public_key(keypair.public_key())?;
```

### Build and Sign a Transaction

```rust
use dytallix_sdk::transaction::TransactionBuilder;
use dytallix_sdk::{DAddr, DytallixKeypair, Token};

let keypair = DytallixKeypair::generate();
let from = DAddr::from_public_key(keypair.public_key())?;
let to = from.clone();

let tx = TransactionBuilder::new()
    .from(from)
    .to(to)
    .amount(1, Token::DRT)
    .nonce(0)
    .build()?;

let signed = tx.sign(&keypair)?;
println!("{}", signed.hash());
```

### Networked Read and Faucet Request

```rust
use dytallix_sdk::client::DytallixClient;
use dytallix_sdk::faucet::FaucetClient;

let client = DytallixClient::testnet().await?;
let faucet = FaucetClient::testnet();
```

The public website gateway supports the account, balance, block, transaction,
fee-estimation, faucet, and submission flows used by the shipped examples.
Those JSON routes are exposed under `/api/blockchain/...` on
`https://dytallix.com`, not the bare website paths. Validator and delegation
reads still require a direct node endpoint today because the public website
gateway does not expose those legacy JSON routes.

### Keystore

```rust
use dytallix_sdk::keystore::Keystore;

let mut keystore = Keystore::open_or_create(Keystore::default_path())?;
```

## Examples

- [`first-keypair.rs`](../examples/first-keypair.rs)
- [`first-transaction.rs`](../examples/first-transaction.rs)
- [`deploy-contract.rs`](../examples/deploy-contract.rs)

Run them from the workspace root:

```bash
cargo run -p dytallix-sdk --example first-keypair
cargo run -p dytallix-sdk --features network --example first-transaction
```

## Errors

Public API errors are represented as `SdkError`. They cover:

- Invalid or unsupported key material
- Address and signature failures
- Serialization problems
- Missing or corrupt keystore state
- Node or faucet availability failures
- Rejected transactions

See [`error.rs`](../crates/dytallix-sdk/src/error.rs) for the current variants.
