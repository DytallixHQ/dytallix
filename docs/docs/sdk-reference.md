# SDK Reference

The Dytallix SDK is a Rust workspace centered on three crates:

- `dytallix-core`
- `dytallix-sdk`
- `dytallix-cli`

This page focuses on the Rust SDK surface used by applications and automation.

## Install

Minimal keypair and address flow:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
```

Network-enabled client and faucet flow:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
```

## Public Types

### `DytallixKeypair`

Primary methods:

- `DytallixKeypair::generate()`
- `DytallixKeypair::generate_slh_dsa()`
- `DytallixKeypair::from_private_key(bytes)`
- `keypair.sign(message)`
- `keypair.public_key()`
- `keypair.private_key()`
- `keypair.scheme()`

Current public-account guidance:

- use `ML-DSA-65` for normal Dytallix accounts
- treat `SLH-DSA` as a special-purpose option, not the default account model

### `DAddr`

Primary methods:

- `DAddr::from_public_key(pubkey)`
- `DAddr::from_str(address)`
- `addr.as_bytes()`
- `addr.as_str()`

Rules:

- only ML-DSA-65 public keys derive a canonical D-Addr
- addresses are Bech32m with the `dytallix` HRP

### `Token`

Enum variants:

- `Token::DGT`
- `Token::DRT`

Public interpretation today:

- `DGT` is used for governance, staking, and current public testnet fees
- `DRT` is the reward token and a first-class transferable asset

### Other Core Data Types

- `Balance`
- `AccountState`
- `FeeEstimate`
- `TransactionReceipt`
- `TransactionStatus`
- `Block`
- `ChainStatus`
- `Validator`
- `Delegation`
- `ContractInfo`
- `FaucetStatus`
- `KeystoreEntry`

## `DytallixClient`

Available with the `network` feature.

Constructors:

- `DytallixClient::new(endpoint)`
- `DytallixClient::testnet()`
- `DytallixClient::local()`

Core methods:

- `get_account(&address)`
- `get_balance(&address)`
- `get_block(id)`
- `get_transaction(hash)`
- `get_chain_status()`
- `submit_transaction(&signed_tx)`
- `simulate_transaction(&tx)`
- `get_validators()`
- `get_delegations(&address)`

Current public-gateway behavior:

- read methods use root RPC endpoints like `/status`, `/account/:address`, and
  `/tx/:hash`
- signed transactions are submitted to `/api/blockchain/submit`
- fee simulation is derived from the gas schedule returned by `/status`
- `get_validators()` and `get_delegations()` still require a direct node
  endpoint today because the public website gateway does not expose those
  legacy JSON routes

## `FaucetClient`

Available with the `network` feature.

Constructors and methods:

- `FaucetClient::new(endpoint)`
- `FaucetClient::testnet()`
- `fund(&address)`
- `fund_dgt(&address)`
- `fund_drt(&address)`
- `status(&address)`

Canonical public endpoint:

```text
https://dytallix.com/api/faucet
```

## `Keystore`

File-backed keystore helpers:

- `Keystore::default_path()`
- `Keystore::open(path)`
- `Keystore::open_or_create(path)`
- `add_keypair(&keypair, name)`
- `get_keypair(name)`
- `list()`
- `active()`
- `set_active(name)`
- `save()`

Default path:

```text
~/.dytallix/keystore.json
```

## Transactions

### `TransactionBuilder`

Builder methods include:

- `from(address)`
- `to(address)`
- `amount(value, token)`
- `gas_limit(c_gas, b_gas)`
- `nonce(value)`
- `data(bytes)`
- `chain_id(value)`
- `fee_micro(value)`
- `memo(value)`
- `build()`

The published SDK builds the live wire-format transaction and supports:

- transfer messages
- arbitrary data messages
- explicit fee override in micro-units

### `Transaction`

Useful methods:

- `with_fee_micro(value)`
- `sign(&keypair)`
- `fee_estimate()`
- `estimate_fee(&client)`
- `with_estimated_fee(&client)`

### `SignedTransaction`

Useful methods:

- `fee_breakdown()`
- `hash()`

Envelope fields submitted to the node:

- `tx`
- `signature`
- `public_key`
- `algorithm`
- `version`

## Example

```rust
use dytallix_sdk::{DAddr, DytallixKeypair, Token};
use dytallix_sdk::client::DytallixClient;
use dytallix_sdk::faucet::FaucetClient;
use dytallix_sdk::transaction::TransactionBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let keypair = DytallixKeypair::generate();
    let address = DAddr::from_public_key(keypair.public_key())?;

    FaucetClient::testnet().fund(&address).await?;

    let client = DytallixClient::testnet().await?;
    let account = client.get_account(&address).await?;

    let tx = TransactionBuilder::new()
        .from(address.clone())
        .to(address.clone())
        .amount(1, Token::DRT)
        .nonce(account.nonce)
        .build()?;

    let (tx, fee) = tx.with_estimated_fee(&client).await?;
    println!("{fee}");

    let signed = tx.sign(&keypair)?;
    let receipt = client.submit_transaction(&signed).await?;
    println!("confirmed tx: {}", receipt.hash);
    Ok(())
}
```

## Error Handling

The SDK uses `SdkError` for:

- address and core-crypto failures
- insufficient balance or gas
- faucet rate limiting or unavailability
- node unavailability
- transaction rejection
- keystore errors
- serialization and I/O failures

The CLI wraps these into more user-friendly messages, but the SDK preserves the
lower-level cause categories for application use.
