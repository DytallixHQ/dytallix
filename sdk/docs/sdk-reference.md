# SDK Reference

[Docs hub](README.md) | [Getting started](getting-started.md) | [Examples](../examples/README.md)

## Crates

| Crate | Purpose | Link |
| --- | --- | --- |
| `dytallix-core` | PQC keypairs, addresses, signatures, hashing, and errors | [`crates/dytallix-core`](../crates/dytallix-core) |
| `dytallix-sdk` | Re-exported core types, ordinary-v2, ordinary-v3 and recovery transactions, encrypted keystore, optional Comet JSON-RPC client | [`crates/dytallix-sdk`](../crates/dytallix-sdk) |
| `dytallix-cli` | End-user command-line workflows built on the SDK | [`crates/dytallix-cli`](../crates/dytallix-cli) |

## Install

Add the SDK from Git:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
```

Add the SDK with the node client:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features comet-rpc
```

## Feature Flags

| Feature | Default | Effect |
| --- | --- | --- |
| `compatibility` | Yes | Enables `dytallix-core/compatibility`: the legacy SPHINCS+ backend and ML-DSA parameter sets other than ML-DSA-65 |
| `comet-rpc` | No | Adds `ordinary_client::CometClient` and `transport`: plain HTTP to a loopback node, or the post-quantum client channel to a pinned endpoint |
| `ordinary-http-only` | No | Local qualification profile: `ordinary_client` over loopback HTTP only |
| `strict-local-mldsa65` | No | Local qualification profile: loopback HTTP and ML-DSA-65 only, without `compatibility` |

`comet-rpc`, `ordinary-http-only` and `strict-local-mldsa65` are mutually
exclusive; build the local profiles with `--no-default-features`. See the
[ordinary CLI](ordinary-cli.md) for the local profiles. No feature uses TLS.

Without `comet-rpc`, the SDK still supports offline key generation, address
derivation, signing, verification, keystore operations, and transaction
building.

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
| `FeeEstimate` | Legacy compute and bandwidth gas split, in micro-denominated fees | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |
| `KeystoreEntry` | A keystore entry's public metadata | [`lib.rs`](../crates/dytallix-sdk/src/lib.rs) |

## Modules

| Module | Purpose | Source |
| --- | --- | --- |
| `ordinary_v2` | Prepare, sign, encode, and check ordinary-v2 transactions and receipts ([Ordinary-v2](ordinary-v2.md)) | [`ordinary_v2.rs`](../crates/dytallix-sdk/src/ordinary_v2.rs) |
| `ordinary_v3` | Prepare and sign ordinary-v3 governance transactions | [`ordinary_v3.rs`](../crates/dytallix-sdk/src/ordinary_v3.rs) |
| `recovery` | Build, sign, assemble, and sponsor recovery transactions ([Recovery CLI](recovery-cli.md)) | [`recovery.rs`](../crates/dytallix-sdk/src/recovery.rs) |
| `keystore` | Encrypted file-backed keystore (version 2): create, open, unlock, migrate, save, list, and active-wallet logic | [`keystore.rs`](../crates/dytallix-sdk/src/keystore.rs) |
| `transaction` | Legacy transaction model: build, sign, hash, and estimate fees locally, with no network calls | [`transaction.rs`](../crates/dytallix-sdk/src/transaction.rs) |
| `ordinary_client` | `CometClient`: CometBFT JSON-RPC queries, CheckTx, and submission | [`ordinary_client.rs`](../crates/dytallix-sdk/src/ordinary_client.rs) |
| `transport` | How requests reach a node: loopback HTTP or the client channel | [`transport.rs`](../crates/dytallix-sdk/src/transport.rs) |
| `error` | SDK error variants for cryptography, serialization, keystore, and network failures | [`error.rs`](../crates/dytallix-sdk/src/error.rs) |

`ordinary_client` and `transport` are compiled with `comet-rpc`, and with the
local profiles, which reach loopback nodes only.

## Common Flows

### Offline Keypair and Address

```rust
use dytallix_sdk::{DAddr, DytallixKeypair};

let keypair = DytallixKeypair::generate();
let address = DAddr::from_public_key(keypair.public_key())?;
```

### Build and Sign a Legacy Transaction

The legacy model makes no network calls:

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

### Query a Node

With `comet-rpc`:

```rust
use dytallix_sdk::ordinary_client::{CometClient, EndpointPin};

// A node on this machine: plain HTTP to a literal loopback address.
let local = CometClient::new("http://127.0.0.1:26657", 1024 * 1024)?;
let profile = local.query_profile().await?;

// A remote node: the post-quantum client channel to the endpoint in its pin file.
let pin = EndpointPin::parse(&std::fs::read("endpoint-pin.json")?)?;
let remote = CometClient::channel(pin, 1024 * 1024)?;
```

The client has no default endpoint and does not fall back from one transport
to the other. HTTPS and plain HTTP to a remote host are refused. Query results
are the node's report, not a consensus proof; see [Ordinary-v2](ordinary-v2.md).

### Keystore

```rust
use dytallix_sdk::keystore::Keystore;
use dytallix_sdk::DytallixKeypair;

// A new version 2 keystore, encrypted under `passphrase` (a `&[u8]`).
// Nothing is written until `save`, which replaces any file at `path`.
let mut keystore = Keystore::create(path.clone(), passphrase)?;
keystore.add_keypair(&DytallixKeypair::generate(), "default")?;
keystore.save()?;

// An existing keystore opens locked; its keys need `unlock`.
let mut keystore = Keystore::open(path)?;
keystore.unlock(passphrase)?;
let keypair = keystore.get_keypair("default")?;
```

`Keystore::default_path()` is `~/.dytallix/keystore.json`, the CLI's keystore.

## Examples

- [`first-keypair.rs`](../examples/first-keypair.rs)

Run it from the workspace root:

```bash
cargo run -p dytallix-sdk --example first-keypair
```

## Errors

Public API errors are represented as `SdkError`. They cover:

- Invalid or unsupported key material
- Address and signature failures
- Insufficient balance or gas
- Serialization problems
- Missing, corrupt, locked, or plaintext keystore state, and a wrong passphrase
- Network mismatches and network failures
- Rejected transactions

See [`error.rs`](../crates/dytallix-sdk/src/error.rs) for the current variants.
The `ordinary_v2`, `ordinary_v3`, `recovery`, `ordinary_client`, and
`transport` modules return `ordinary_v2::Error`.
