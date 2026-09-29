# Getting Started

[Docs hub](README.md) | [Project README](../README.md) | [Examples](../examples/README.md)

This is the mainnet candidate SDK and CLI for the consensus chain. It contains
no public testnet client.

## Install Paths

The SDK is currently consumed from Git, not crates.io.

Add the library crate:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
```

Add the library crate with the node client (`ordinary_client::CometClient`):

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features comet-rpc
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

## Node Client

The `comet-rpc` feature adds `ordinary_client::CometClient`, a CometBFT
JSON-RPC client for the consensus chain:

```rust
use dytallix_sdk::ordinary_client::{CometClient, EndpointPin};

// A node on this machine: plain HTTP to a literal loopback address.
let local = CometClient::new("http://127.0.0.1:26657", 1024 * 1024)?;
let profile = local.query_profile().await?;

// A remote node: the post-quantum client channel to the endpoint in its pin file.
let pin = EndpointPin::parse(&std::fs::read("endpoint-pin.json")?)?;
let remote = CometClient::channel(pin, 1024 * 1024)?;
```

There is no default endpoint. HTTPS and plain HTTP to a remote host are
refused. See [Ordinary-v2](ordinary-v2.md) for preparing and signing
transactions.

## CLI Quickstart

The CLI stores its state under `~/.dytallix/`.

Create a wallet. The first wallet creates the encrypted keystore and asks for
its passphrase:

```bash
dytallix wallet create --name default
```

Pin the consensus chain. Take the network, chain ID and genesis digest from a
source you trust, never from the node itself. For a node on this machine, use
its loopback URL:

```bash
dytallix config pin-chain --endpoint http://127.0.0.1:26657 --network <mainnet|testnet|development> \
  --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
```

For a remote node, pass the endpoint's pin file, which its operator publishes:

```bash
dytallix config pin-chain --endpoint ./endpoint-pin.json --network <mainnet|testnet|development> \
  --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
```

`pin-chain` asks the node which chain it reports and refuses a mismatch.

Inspect the wallet and its balance:

```bash
dytallix wallet info
dytallix balance
```

Once a chain is pinned, `wallet info` prints the wallet's address on that
chain.

There is no faucet. An account receives funds at genesis or by a transfer from
a funded account.

Send a transfer:

```bash
dytallix send --to <address> --amount 1.5 --gas-limit <n> --maximum-fee-udrt <n>
```

`--gas-limit` and `--maximum-fee-udrt` are required; the CLI never chooses
them. `send` signs, submits, and waits up to `--wait-seconds` (default 30) for
the committed receipt. A `send` to an address with no account creates it and
burns the chain's account creation fee.

See the [CLI reference](cli-reference.md) for `stake`, `governance`,
`ordinary` and `recovery`.

## Local Files

- Keystore: `~/.dytallix/keystore.json`
- Pinned chain: `~/.dytallix/chain.json`

## Next Steps

- Read [Core concepts](core-concepts.md) for tokens, addresses, fees, and how the CLI reaches a node.
- Read [SDK reference](sdk-reference.md) for the Rust API layout.
- Read [CLI reference](cli-reference.md) for command-by-command examples.
- Read [FAQ](faq.md) for current operational caveats.
