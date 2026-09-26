# Dytallix Node

Consensus application and consensus engine for Dytallix mainnet.

Mainnet is not launched. See [../launch/](../launch/) for the specification,
launch gates and open decisions.

The public testnet runs an older, separate node. Its source is not part of this
tree.

## Components

| Path | Contents |
| --- | --- |
| [`dytallix-fast-launch/node`](dytallix-fast-launch/node) | Rust consensus application (package `dytallix-fast-node`): transaction admission, execution, settlement, validator lifecycle, rewards, governance, emergency controls and upgrades |
| [`consensus/cometbft`](consensus/cometbft) | CometBFT v0.40.0 fork with ML-DSA-65 validator keys and a PQC peer transport, plus the Go ABCI bridge, fixture generator, verifier and engine |
| [`consensus/owner-guard`](consensus/owner-guard), [`consensus/root-authorization`](consensus/root-authorization) | Go process-ownership and root-authorization helpers |
| [`consensus/pqc-http-adapter`](consensus/pqc-http-adapter) | Local HTTP adapter (separate Cargo workspace) |
| [`crates/`](crates) | Shared crates: adaptive emission, gas, native supervisor, protocol types, release runtime, runtime crypto (FIPS 204 ML-DSA-65), signature policy, storage |
| [`deploy/pqc-engine`](deploy/pqc-engine) | Engine service templates and Linux build notes |
| [`deploy/genesis*.json`](deploy) | Development genesis fixtures used by tests |
| [`tools/`](tools), [`scripts/`](scripts) | Preparation, boundary and dependency-profile checks |

## Build

Requires Rust 1.88.0 (pinned in `rust-toolchain.toml`) and Go 1.25 or later.

```bash
cargo build --workspace --all-targets --locked
cargo test --workspace --locked

cd consensus/cometbft
go test -mod=readonly ./...
```

See [Build and run](docs/build-and-run.md) for the consensus binaries and
checks.

## Documentation

- [Docs hub](docs/README.md)
- [Build and run](docs/build-and-run.md)
- [Repository map](docs/repository-map.md)
- [Modular node architecture](docs/architecture/modular-node.md)
- [Component contracts and batch records](docs/mainnet/)
- [CometBFT integration](consensus/cometbft/README.md)
- [PQC engine deployment](deploy/pqc-engine/README.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
