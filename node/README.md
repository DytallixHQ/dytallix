# Dytallix Node

Public node, RPC, and backend source for the Dytallix testnet.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

This repository is the canonical public node and backend source for the live
Dytallix testnet.

The machine-readable public capability source of truth currently lives in this
repository at `docs/public-capabilities.json` and should be kept aligned with
runtime behavior before broader publication.

Compatible node deployments serve that same contract at `GET /api/capabilities`
so clients can discover the active public surface without scraping docs.

## Repository Role

- Role: public node and backend source
- Current publication state: canonical public node/backend source with clean-checkout production provenance
- Production status: the public deployment now runs from a clean checkout rooted
  at `/opt/dytallix-node`, built from this repository and launched from
  `/opt/dytallix-node/target/release/dytallix-fast-node`

## Quick Links

- [Docs hub](docs/README.md)
- [Capability manifest](docs/public-capabilities.json)
- [Public deployment evidence](docs/public-deployment-evidence.md)
- [Repository map](docs/repository-map.md)
- [Build and run](docs/build-and-run.md)
- [RPC and API docs](docs/rpc-and-apis.md)
- [FAQ](docs/faq.md)
- [Deployment separation audit](docs/deployment-separation-audit.md)
- [Fast node RPC reference](dytallix-fast-launch/node/README_RPC.md)
- [PQC implementation](dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
- [Secrets management](blockchain-core/SECRETS_README.md)
- [PulseGuard API draft](blockchain-core/src/risk/pulseguard/api/OPENAPI.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)
- [License](LICENSE)

## What This Repository Contains

The workspace is split into four main packages:

- [`dytallix-fast-launch/node`](dytallix-fast-launch/node) - public RPC node and execution engine
- [`blockchain-core`](blockchain-core) - shared chain logic, consensus, secrets, risk, and runtime code
- [`pqc-crypto`](pqc-crypto) - post-quantum cryptography primitives and CLI tools
- [`smart-contracts`](smart-contracts) - contract runtime, bridges, and examples

See [Repository map](docs/repository-map.md) for the package names, binaries,
and notable subpaths.

## Why This Repository Matters

This published tree includes the server-side fixes that make the public signed
transaction path usable end to end:

- signed ML-DSA-65 transactions are accepted by the live node
- the signed `fee` field is converted into execution gas at submit time
- `/status` exposes public gas parameters for SDK and CLI fee estimation

## Prerequisites

Install [Rust](https://www.rust-lang.org/tools/install) with `rustup`. That
provides the Rust toolchain and `cargo` used by the workspace build and run
commands in this repository.

## Build Quickstart

Build the full workspace:

```bash
cargo build --workspace --locked
```

Build the public RPC node:

```bash
cargo build -p dytallix-fast-node --bin dytallix-fast-node --release --locked
```

Run the public RPC node locally:

```bash
cargo run -p dytallix-fast-node --bin dytallix-fast-node --release
```

See [Build and run](docs/build-and-run.md) for crate-specific commands and
entrypoints.

## Public API Surfaces

The main API and protocol references are already in the repository:

- [Fast node RPC reference](dytallix-fast-launch/node/README_RPC.md)
- [PulseGuard API draft](blockchain-core/src/risk/pulseguard/api/OPENAPI.md)
- [PQC implementation notes](dytallix-fast-launch/node/PQC_IMPLEMENTATION.md)
- [Secrets management guide](blockchain-core/SECRETS_README.md)

The shorter repo-level summary is in [RPC and API docs](docs/rpc-and-apis.md).

## Clean Deployment Notes

- Runtime data, RocksDB files, launch evidence, local keys, Finder metadata,
  and temporary backup artifacts are intentionally excluded from the public repo.
- `pqc-crypto` referenced a missing local dev dependency on the server
  (`../interoperability`). That dev-only dependency was removed here so the
  published tree builds cleanly.
- The clean checkout deployment path in [Build and run](docs/build-and-run.md)
  is now the active production path.

## Related Repositories

- [dytallix-sdk](https://github.com/DytallixHQ/dytallix-sdk)
- [dytallix.com](https://dytallix.com) - hosted public website and explorer runtime surface
- [dytallix-explorer](https://github.com/DytallixHQ/dytallix-explorer) - explorer surface documentation repo
- [dytallix-faucet](https://github.com/DytallixHQ/dytallix-faucet) - canonical public faucet backend source
- [DytallixHQ](https://github.com/DytallixHQ)
