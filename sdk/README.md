# Dytallix SDK

[![Rust](https://img.shields.io/badge/Rust-stable-000000?logo=rust)](https://www.rust-lang.org/tools/install)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Status: Testnet](https://img.shields.io/badge/Status-Testnet-0a7f5a)](https://dytallix.com)
[![CI](https://github.com/DytallixHQ/dytallix-sdk/actions/workflows/ci.yml/badge.svg)](https://github.com/DytallixHQ/dytallix-sdk/actions/workflows/ci.yml)

Official Rust SDK and CLI for the Dytallix public testnet.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

This repository contains the Rust workspace for the core cryptography crate,
the application SDK, and the `dytallix` CLI.

## Repository Role

- Role: public SDK and CLI source
- Current publication state: canonical public client source for install,
  onboarding, and runtime capability consumption
- Important boundary: this repository describes and consumes the public surface,
    but it does not replace the separate publication boundaries for the website
    frontend, explorer frontend, or faucet backend source

## Quick Links

- [Docs hub](docs/README.md)
- [Capability manifest](docs/public-capabilities.json)
- [Getting started](docs/getting-started.md)
- [Core concepts](docs/core-concepts.md)
- [SDK reference](docs/sdk-reference.md)
- [CLI reference](docs/cli-reference.md)
- [FAQ](docs/faq.md)
- [Examples](examples/README.md)
- [Releases](https://github.com/DytallixHQ/dytallix-sdk/releases)
- [CI workflow](.github/workflows/ci.yml)
- [Public smoke workflow](.github/workflows/public-smoke.yml)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [License](LICENSE)

## What Is Here

- [`crates/dytallix-core`](crates/dytallix-core) - cryptographic primitives
- [`crates/dytallix-sdk`](crates/dytallix-sdk) - Rust SDK library
- [`crates/dytallix-cli`](crates/dytallix-cli) - public testnet CLI
- [`docs/`](docs/README.md) - repository documentation and capability notes
- [`examples/`](examples/README.md) - runnable examples for keypair, transfer, and contract flows

All signing uses ML-DSA-65 (FIPS 204). All addresses are canonical Bech32m.
Only PQC-native accounts are supported.

## Prerequisites

Install [Rust](https://www.rust-lang.org/tools/install) with `rustup`. That
provides the Rust toolchain, `cargo`, and target management used throughout the
Dytallix Rust repositories.

If you plan to build WASM contracts locally, add the standard target:

```bash
rustup target add wasm32-unknown-unknown
```

## Install

The SDK is not currently published on crates.io. Use the Git repository:

```bash
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git
cargo add dytallix-sdk --git https://github.com/DytallixHQ/dytallix-sdk.git --features network
cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
```

Build from source:

```bash
cargo build --release --bin dytallix
```

Release tags matching `v*` build downloadable CLI archives for Linux, macOS,
and Windows through GitHub Actions.

## Developer Path

These are the three developer milestones this repository is optimized for:

1. **First keypair: under 60 seconds**

    Generate your first ML-DSA-65 keypair and print a D-Addr:

    ```bash
    git clone https://github.com/DytallixHQ/dytallix-sdk
    cd dytallix-sdk
    cargo run -p dytallix-sdk --example first-keypair
    ```

    Start here: [first-keypair example](examples/first-keypair.rs)

2. **First transaction on testnet: 2-3 minutes**

    Create a funded sender wallet, create a separate recipient wallet, then
    submit and verify a real transaction:

    ```bash
    cargo install --git https://github.com/DytallixHQ/dytallix-sdk.git dytallix-cli --bin dytallix
    dytallix init
    dytallix wallet create --name recipient
    dytallix wallet list
    dytallix wallet switch default
    dytallix send <recipient-daddr> 100
    dytallix wallet switch recipient
    dytallix balance
    ```

    Use a different recipient address than the one printed by `dytallix init`
    so you do not self-send. The `send` command waits for the submitted
    transaction to leave `Pending` when the public receipt route is already
    indexing.

    Continue with: [first-transaction example](examples/first-transaction.rs) · [Explorer](https://dytallix.com/build/blockchain) · [Releases](https://github.com/DytallixHQ/dytallix-sdk/releases)

3. **First contract build: under 15 minutes**

    Build a minimal WASM contract now. The default public gateway accepts
    `POST /contracts/deploy` and `POST /contracts/call`; use a direct node
    endpoint or a local node only when you want local testing or custom
    infrastructure.

    ```bash
    rustup target add wasm32-unknown-unknown
    cargo build --manifest-path examples/contracts/minimal_contract/Cargo.toml --target wasm32-unknown-unknown --release
    ```

    Continue with: [deploy-contract example](examples/deploy-contract.rs) · [dytallix-contracts](https://github.com/DytallixHQ/dytallix-contracts) · [Docs](https://dytallix.com/docs)

## Public Testnet Scope

The default public endpoint is `https://dytallix.com`.

Supported on the public website gateway today:

- keypair and wallet generation
- faucet funding and cooldown/status checks
- balance reads and transfers
- chain status, block, and transaction reads
- basic contract call, query, info, and events
- governance proposal reads
- staking balance reads

Not public-complete on the default website gateway:

- staking writes such as delegate, undelegate, and claim
- governance writes such as vote and propose
- validator/delegation legacy JSON reads
- advanced or operator-only paths

For local development or direct-node testing, point the CLI at a custom endpoint
with `DYTALLIX_ENDPOINT` or `dytallix config set endpoint ...`.

## First CLI Session

```bash
dytallix init
dytallix balance
dytallix faucet status
dytallix send <daddr> 100
```

If you have a compiled contract artifact, you can also deploy on the default
public testnet profile:

```bash
dytallix contract deploy <path-to-your-contract.wasm>
```

Use a direct node endpoint or local node only when you want local testing or a
custom RPC base.

See [Getting started](docs/getting-started.md) and the
[CLI reference](docs/cli-reference.md) for the full flow.

## Repo Boundaries

This repository ships the Rust SDK, core cryptography crate, and the
`dytallix` CLI.

It does include the client code that talks to the live faucet endpoint, but it
does not contain the faucet backend implementation.

## Documentation Map

- [Docs hub](docs/README.md) - overview of every repo documentation page
- [Getting started](docs/getting-started.md) - install, first keypair, first CLI session
- [Core concepts](docs/core-concepts.md) - tokens, addresses, gas, keystore, network profiles
- [SDK reference](docs/sdk-reference.md) - crate surface and common Rust workflows
- [CLI reference](docs/cli-reference.md) - command map and examples
- [FAQ](docs/faq.md) - operational and product questions
- [Examples](examples/README.md) - runnable examples and prerequisites

## DytallixHQ Repositories

- [dytallix-sdk](https://github.com/DytallixHQ/dytallix-sdk) - this repository
- [dytallix-node](https://github.com/DytallixHQ/dytallix-node) - public node and runtime source
- [dytallix-contracts](https://github.com/DytallixHQ/dytallix-contracts) - protocol contracts
- [dytallix-docs](https://github.com/DytallixHQ/dytallix-docs) - broader documentation
- [dytallix-explorer](https://github.com/DytallixHQ/dytallix-explorer) - explorer surface documentation repo
- [dytallix-faucet](https://github.com/DytallixHQ/dytallix-faucet) - canonical public faucet backend source
- [DytallixHQ](https://github.com/DytallixHQ)

## External Links

- [Website](https://dytallix.com)
- [Documentation site](https://dytallix.com/docs)
- [Discord](https://discord.gg/eyVvu5kmPG)
- [Explorer app](https://dytallix.com/build/blockchain)
- [Faucet API](https://dytallix.com/api/faucet)
