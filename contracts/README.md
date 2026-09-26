# Dytallix Contracts

Reference WASM contracts, protocol state machines, and example contract patterns for Dytallix.

This repository contains the public reference contracts toolkit for the Dytallix contract layer:

- core tokenomics modules for DGT, DRT, and emission control
- reference staking, governance, and algorithm-registry contracts
- the shared WASM runtime, bridge, storage, gas, and security utilities
- standalone example contracts that show how to compose the modules
- contributor docs, testing guidance, and CI support

## Prerequisites

Install [Rust](https://www.rust-lang.org/tools/install) with `rustup`. That
provides the Rust toolchain and `cargo` used by the contract workspace.

If you plan to build deployable WASM artifacts locally, add the standard
target:

```bash
rustup target add wasm32-unknown-unknown
```

## Quick Start

```bash
cargo fmt
cargo test
cargo test --manifest-path examples/ping/Cargo.toml
cargo test --manifest-path examples/counter/Cargo.toml
cargo test --manifest-path examples/reward_splitter/Cargo.toml
cargo test --manifest-path examples/algorithm_guard/Cargo.toml
```

## Repository Map

- [docs/index.md](docs/index.md) - documentation landing page
- [docs/architecture.md](docs/architecture.md) - crate layout, execution model, and contract boundaries
- [docs/reference-contracts.md](docs/reference-contracts.md) - module-by-module contract reference
- [docs/examples.md](docs/examples.md) - example contract walkthroughs
- [docs/testing.md](docs/testing.md) - local development and CI commands
- [CONTRIBUTING.md](CONTRIBUTING.md) - contribution workflow
- [SECURITY.md](SECURITY.md) - vulnerability reporting and review scope
- [.github/workflows/ci.yml](.github/workflows/ci.yml) - CI checks for formatting and tests

## Reference Contracts

- [src/tokenomics/dgt_token.rs](src/tokenomics/dgt_token.rs) - fixed-supply governance token
- [src/tokenomics/drt_token.rs](src/tokenomics/drt_token.rs) - elastic reward token with burn and emission hooks
- [src/tokenomics/emission_controller.rs](src/tokenomics/emission_controller.rs) - adaptive emission controller and reward-pool distribution
- [src/staking.rs](src/staking.rs) - validator registration, delegation, slashing, and reward-index accounting
- [src/governance.rs](src/governance.rs) - deposits, voting, quorum checks, timelock, and execution routing
- [src/algorithm_registry.rs](src/algorithm_registry.rs) - cryptographic algorithm lifecycle management with circuit breaker support
- [src/runtime.rs](src/runtime.rs) - WASM execution runtime and host-side contract orchestration
- [src/security/](src/security) - gas attack analysis, scanning, fuzzing, and audit helpers
- [src/storage_optimizer.rs](src/storage_optimizer.rs) - storage caching, compression, and access-pattern analysis

## Canonical Surface

The canonical public modules are the ones exported from [src/lib.rs](src/lib.rs):

- [src/runtime.rs](src/runtime.rs) for the shared WASM runtime
- [src/cosmos_bridge.rs](src/cosmos_bridge.rs) for the reference Cosmos bridge
- `oracle` as the dependency-light oracle module re-exported from the crate root

Historical comparison files such as `runtime_backup.rs`, `runtime_clean.rs`,
and `runtime_simple.rs` are not part of the supported public API surface.

## Example Contracts

- [examples/ping](examples/ping) - minimal exported-method contract for signed call smoke tests
- [examples/counter](examples/counter) - minimal stateful counter contract with owner-guarded reset
- [examples/reward_splitter](examples/reward_splitter) - emission splitting contract layered on the staking module
- [examples/algorithm_guard](examples/algorithm_guard) - algorithm attestation gate built on the registry contract
- [examples/README.md](examples/README.md) - example index and commands

## Support Files

- [CHANGELOG.md](CHANGELOG.md) - release notes for the public contracts repo
- [Makefile](Makefile) - common formatting and test commands
- [.gitignore](.gitignore) - build artifact exclusions

## Notes

This repository is the reference contracts and examples surface for Dytallix. It is not a claim that every module here is already wired into the current public testnet deployment path. The authoritative protocol and API notes live in [dytallix-docs](https://github.com/DytallixHQ/dytallix-docs).

