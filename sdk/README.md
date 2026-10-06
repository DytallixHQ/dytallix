# Dytallix SDK

[![Rust](https://img.shields.io/badge/Rust-stable-000000?logo=rust)](https://www.rust-lang.org/tools/install)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-yellow.svg)](LICENSE)
[![Status: Mainnet candidate](https://img.shields.io/badge/Status-Mainnet%20candidate-0a7f5a)](https://dytallix.com)
[![CI](https://github.com/DytallixHQ/dytallix-sdk/actions/workflows/ci.yml/badge.svg)](https://github.com/DytallixHQ/dytallix-sdk/actions/workflows/ci.yml)

Official Rust SDK and CLI for the Dytallix consensus chain.

This is the mainnet candidate SDK. It contains no public testnet client: no
faucet, no testnet REST endpoint and no contract deployment. The CLI reaches a
consensus-chain node (CometBFT JSON-RPC) over loopback HTTP on this machine,
or over the post-quantum client channel for a remote node. There is no TLS.

This repository contains the Rust workspace for the core cryptography crate,
the application SDK, and the `dytallix` CLI.

## Repository Role

- Role: public SDK and CLI source
- Current publication state: mainnet candidate client source, installed from
  Git
- Important boundary: this repository holds client code only. Node operators
  publish their own endpoint pin files.

## Quick Links

- [Docs hub](docs/README.md)
- [Getting started](docs/getting-started.md)
- [Core concepts](docs/core-concepts.md)
- [SDK reference](docs/sdk-reference.md)
- [CLI reference](docs/cli-reference.md)
- [FAQ](docs/faq.md)
- [Examples](examples/README.md)
- [Releases](https://github.com/DytallixHQ/dytallix-sdk/releases)
- [CI workflow](.github/workflows/ci.yml)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [License](LICENSE)

## What Is Here

- [`crates/dytallix-core`](crates/dytallix-core) - cryptographic primitives
- [`crates/dytallix-sdk`](crates/dytallix-sdk) - Rust SDK library
- [`crates/dytallix-cli`](crates/dytallix-cli) - `dytallix` CLI for the consensus chain
- [`docs/`](docs/README.md) - repository documentation
- [`examples/`](examples/README.md) - the runnable first-keypair example

All signing uses ML-DSA-65 (FIPS 204). All addresses are canonical Bech32m.
Only PQC-native accounts are supported.

## Prerequisites

Install [Rust](https://www.rust-lang.org/tools/install) with `rustup`. That
provides the Rust toolchain, `cargo`, and target management used throughout the
Dytallix Rust repositories.

## Install

The SDK is not currently published on crates.io. Use the Git repository:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features comet-rpc
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

The `comet-rpc` feature adds the node client, `ordinary_client::CometClient`.
The CLI has the consensus-chain commands: `wallet`, `balance`, `send`,
`ordinary`, `recovery`, `stake`, `governance`, `crypto` and `config`.

Build from source:

```bash
cargo build --release --bin dytallix
```

Release tags matching `v*` build downloadable CLI archives for Linux, macOS,
and Windows through GitHub Actions.

## Developer Path

1. **First keypair: under 60 seconds**

    Generate your first ML-DSA-65 keypair and print a D-Addr:

    ```bash
    git clone https://github.com/DytallixHQ/dytallix-sdk
    cd dytallix-sdk
    cargo run -p dytallix-sdk --example first-keypair
    ```

    Start here: [first-keypair example](examples/first-keypair.rs)

2. **First transaction on the consensus chain**

    You need a node to reach and a funded account. There is no faucet: an
    account receives funds at genesis or by a transfer from a funded account.

    ```bash
    cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
    dytallix wallet create --name default
    dytallix config pin-chain --endpoint http://127.0.0.1:26657 --network <network> \
      --chain-id <chain-id> --genesis-digest <sha256-of-genesis-hex>
    dytallix wallet info
    dytallix balance
    dytallix send --to <address> --amount 1.5 --gas-limit <n> --maximum-fee-udrt <n>
    ```

    `--network` is `mainnet`, `testnet` or `development`. Take it, the chain
    ID and the genesis digest from a source you trust, never from the node
    itself. For a remote node, pass the endpoint's pin file as `--endpoint`.
    After `pin-chain`, `wallet info` prints the wallet's address on the
    pinned chain.

## Reaching a Node

There is no default endpoint. `--endpoint`, on `config pin-chain` and on each
command that reads or writes, takes one of two forms:

- a node on this machine: plain HTTP to a literal loopback address, such as
  `http://127.0.0.1:26657`;
- a remote node: the path of the endpoint's pin file, which its operator
  publishes. Requests cross the post-quantum client channel: ML-KEM-768 key
  exchange, then an ML-DSA-65 signature by the endpoint's pinned key.

HTTPS and plain HTTP to a remote host are refused. `config pin-chain` stores
the chain in `~/.dytallix/chain.json`.

For a browser wallet, run `dytallix gateway serve --listen 127.0.0.1:4173` on
the same machine. It relays the page's JSON-RPC to the pinned chain, and can
serve a wallet bundle pinned by digest. See the
[CLI reference](docs/cli-reference.md#gateway).

See [Getting started](docs/getting-started.md) and the
[CLI reference](docs/cli-reference.md) for the full flow.

## Repo Boundaries

This repository ships the Rust SDK, core cryptography crate, and the
`dytallix` CLI.

It does not contain the node.

## Documentation Map

- [Docs hub](docs/README.md) - overview of every repo documentation page
- [Getting started](docs/getting-started.md) - install, first keypair, first CLI session
- [Core concepts](docs/core-concepts.md) - tokens, addresses, fees, keystore, reaching a node
- [SDK reference](docs/sdk-reference.md) - crate surface and common Rust workflows
- [CLI reference](docs/cli-reference.md) - command map and examples
- [FAQ](docs/faq.md) - operational and product questions
- [Examples](examples/README.md) - runnable examples and prerequisites

## DytallixHQ Repositories

- [dytallix-sdk](https://github.com/DytallixHQ/dytallix-sdk) - this repository
- [dytallix-node](https://github.com/DytallixHQ/dytallix-node) - public node and runtime source
- [dytallix-docs](https://github.com/DytallixHQ/dytallix-docs) - broader documentation
- [dytallix-explorer](https://github.com/DytallixHQ/dytallix-explorer) - explorer surface documentation repo
- [DytallixHQ](https://github.com/DytallixHQ)

## External Links

- [Website](https://dytallix.com)
- [Documentation site](https://dytallix.com/docs)
- [Discord](https://discord.gg/eyVvu5kmPG)
