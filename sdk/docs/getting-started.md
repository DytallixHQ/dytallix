# Getting Started

[Docs hub](README.md) | [Project README](../README.md) | [Examples](../examples/README.md)

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Install Paths

The SDK is currently consumed from Git, not crates.io.

Add the library crate:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
```

Add the library crate with the network client and faucet support:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
```

Install the CLI:

```bash
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

Build from a local clone:

```bash
cargo build --all
```

## First Keypair

Generate an ML-DSA-65 keypair and print a D-Addr:

```rust
use dytallix_sdk::{DAddr, DytallixKeypair};

fn main() {
    let keypair = DytallixKeypair::generate();
    let addr = DAddr::from_public_key(keypair.public_key()).unwrap();
    println!("{addr}");
}
```

If you cloned this repository, you can run the same flow directly:

```bash
cargo run -p dytallix-sdk --example first-keypair
```

## Network Features

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
```

If you cloned this repository, the network example runs with:

```bash
cargo run -p dytallix-sdk --features network --example first-transaction
```

Minimal networked flow in Rust:

```rust
use dytallix_sdk::client::DytallixClient;
use dytallix_sdk::faucet::FaucetClient;

let client = DytallixClient::testnet().await?;
let faucet = FaucetClient::testnet();
```

The public testnet surface is still evolving, but the SDK now targets the live
read, faucet, and transaction submission routes exposed from
`https://dytallix.com`.

## CLI Quickstart

The CLI stores its state under `~/.dytallix/`.

Create a wallet, persist it to the keystore, and request faucet funds:

```bash
dytallix init
```

Inspect the active wallet:

```bash
dytallix wallet info
dytallix balance
```

When you use the default public endpoint at `https://dytallix.com`, manual
checks can use root routes such as `/status`, `/balance/<daddr>`,
`/account/<daddr>`, and `/submit`. Compatibility aliases are also available on
`/api/status` and `/api/blockchain/...`.

Check faucet eligibility:

```bash
dytallix faucet status
```

The canonical public faucet currently grants `10 DGT` and `100 DRT` per
successful request, enforces a `60` second cooldown, and caps usage at `20`
requests per hour.

Public staking and governance writes are disabled on the default public website
gateway. Use a local node or direct node endpoint for those experimental write
paths.

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
The `send` command waits for the public `/tx/<hash>` route to leave `Pending`
when that route is already indexing. If the recipient balance still shows `0`
immediately after confirmation, run `dytallix balance` again after a moment.

Prepare a first contract deployment:

```bash
dytallix contract deploy ./my_contract.wasm
```

The default testnet profile already targets `https://dytallix.com`, and the
public gateway accepts `POST /contracts/deploy` on that endpoint.

If you want to test against a direct node endpoint or a local node instead,
override the active endpoint:

```bash
dytallix config set endpoint http://localhost:3030
```

To run a local node from this repository checkout:

```bash
./start-local.sh
dytallix config network local
```

After deploy, verify the indexed contract metadata with:

```bash
dytallix contract info <contract-address>
```

On the public testnet gateway, `dytallix contract info <contract-address>` is the
canonical verification path if `/tx/<hash>` indexing lags behind contract metadata.

## Local Files

- Keystore: `~/.dytallix/keystore.json`
- CLI config: `~/.dytallix/config.json`

## Next Steps

- Read [Core concepts](core-concepts.md) for tokens, addresses, gas, and network profiles.
- Read [SDK reference](sdk-reference.md) for the Rust API layout.
- Read [CLI reference](cli-reference.md) for command-by-command examples.
- Read [FAQ](faq.md) for current operational caveats.
