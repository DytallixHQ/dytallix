# Getting Started

This page gets you from zero to a funded Dytallix address and a confirmed
transaction.

## What You Need

- Rust and Cargo
- network access to `https://dytallix.com`
- a few minutes for the faucet and first transaction flow

## Fastest Verified Path: Rust SDK

Install the SDK with network support:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
cargo add tokio --features macros,rt-multi-thread
```

Then run a minimal end-to-end example:

```rust
use dytallix_sdk::{DAddr, DytallixKeypair, Token};
use dytallix_sdk::client::DytallixClient;
use dytallix_sdk::faucet::FaucetClient;
use dytallix_sdk::transaction::TransactionBuilder;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let keypair = DytallixKeypair::generate();
    let address = DAddr::from_public_key(keypair.public_key())?;

    let faucet = FaucetClient::testnet();
    let funded = faucet.fund(&address).await?;
    println!("Funded: {} DGT / {} DRT", funded.dgt, funded.drt);

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
    println!("Hash: {}", receipt.hash);
    println!("Status: {:?}", receipt.status);
    Ok(())
}
```

This flow was verified against the public testnet on April 6, 2026.

The SDK is not currently published on crates.io, so the Git install path above
is the canonical install path.

## CLI Quickstart

Install the CLI:

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

Initialize a local keystore and request faucet funds:

```bash
dytallix init
```

Useful follow-up commands:

```bash
dytallix wallet info
dytallix balance
dytallix faucet status
```

Send a test transfer:

```bash
dytallix wallet create --name recipient
dytallix wallet switch recipient
dytallix wallet info
dytallix wallet switch default
dytallix send <recipient-daddr> 100
dytallix wallet switch recipient
dytallix balance
```

Use a different recipient address than the one created by `dytallix init`.
The current public testnet charges gas in `DGT`, so even a `DRT` transfer needs
some `DGT` in the sending wallet. The `send` command waits for the public
transaction receipt route to leave `Pending` when that route is already
indexing.

## Faucet

Canonical faucet base:

```text
https://dytallix.com/api/faucet
```

Current reported limits on April 17, 2026:

- `10 DGT`
- `100 DRT`
- `60` second cooldown
- `20` requests per hour

Example request:

```bash
curl -X POST https://dytallix.com/api/faucet/request \
  -H 'content-type: application/json' \
  -d '{
    "address": "<D-ADDR>"
  }'
```

## Public Endpoints You Will Use Most

- Node status: `https://dytallix.com/status`
- Account lookup: `https://dytallix.com/account/<D-ADDR>`
- Balance lookup: `https://dytallix.com/balance/<D-ADDR>`
- Explorer page: `https://dytallix.com/build/blockchain`
- Explorer API: `https://dytallix.com/api/blockchain/status`

## What To Read Next

- [`core-concepts.md`](core-concepts.md)
- [`cli-reference.md`](cli-reference.md)
- [`contract-quickstart.md`](contract-quickstart.md)
- [`sdk-reference.md`](sdk-reference.md)
- [`rpc-reference.md`](rpc-reference.md)
