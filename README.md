# Dytallix mainnet

This is the Dytallix mainnet candidate. It began as the `mainnet/` folder of
exocognosis/dytallix, separate from the testnet and product code there, and
moved here with its history on 5 October 2026
([release/MOVE.md](release/MOVE.md)).

**Status: NO GO.** Mainnet is not launched and no launch is authorized. The
35 launch gates are in [launch/MAINNET_GATE_MASTER.md](launch/MAINNET_GATE_MASTER.md),
the decisions in [launch/DECISIONS_REQUIRED.md](launch/DECISIONS_REQUIRED.md)
and the order of work in [launch/GENESIS_FRAMEWORK.md](launch/GENESIS_FRAMEWORK.md).

## Layout

| Folder | Contents |
|---|---|
| [node/](node/) | Consensus application (`dytallix-fast-node`), CometBFT v0.40.0 fork with PQC transport (`node/consensus/cometbft`), PQC HTTP adapter, native supervisor, adaptive emission and storage crates |
| [sdk/](sdk/) | Rust SDK and `dytallix` CLI, including the ordinary-v2 client and browser crate |
| [pqc/](pqc/) | PQC primitives (ML-DSA, SLH-DSA, ML-KEM, FN-DSA). The node is the qualification authority. |
| [contracts/](contracts/) | WASM reference contracts: DGT, DRT, emission, staking, governance, algorithm registry. Not part of the consensus build; nothing here runs on the chain. |
| [docs/](docs/) | Public documentation source (MkDocs). Mostly written for the public testnet; the tokenomics, security model, CLI reference, contract quickstart and whitepaper errata pages describe the mainnet candidate. |
| [launch/](launch/) | Launch gates, decision register and P01 approvals, genesis framework, trust model, production values, custody, host and operations records, specifications and tokenomics |
| [release/](release/) | Reproducible release build, release manifest writer and the repository move |

The consensus build has no contract runtime and no cross-chain bridge. It
runs ordinary, governance and recovery transactions only. Mainnet v1
launches with the current build. A contract runtime, bridges, the Airlock, a
liquidity bootstrapping pool, wrapped USDC, external oracles, gRPC and
treasury spending are POST MAINNET (D07-Q01, P01, 29 September 2026).
`dytallix-comet-bridge` in `node/consensus/cometbft` is the adapter between
the consensus engine and the application, not a cross-chain bridge.

The testnet faucet moved to
[`testnet/faucet`](https://github.com/exocognosis/dytallix/tree/main/testnet/faucet)
in exocognosis/dytallix on 29 September 2026. It is testnet-only and served
over TLS, and mainnet has no faucet.

## Build

Toolchains: Rust 1.88.0 (pinned by each workspace's `rust-toolchain.toml`) and Go 1.25.

From this folder:

```sh
cd node
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
```

The consensus engine lives in `node/consensus/cometbft`:

```sh
cd node/consensus/cometbft
go test -mod=readonly ./...
```

See `node/consensus/cometbft/README.md` and `PQC_ENGINE_INTEGRATION.md` there.

CI runs all of the above on every change except documentation-only ones
(`.github/workflows/mainnet.yml`),
and [release/README.md](release/README.md) describes the reproducible release
build.

## Contributing and license

See [CONTRIBUTING.md](CONTRIBUTING.md): outside contributions are signed off
under the [Developer Certificate of Origin](DCO). Report vulnerabilities
privately ([SECURITY.md](SECURITY.md)). Dytallix is dual licensed under the
[MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE),
at your option; third-party code keeps its own license.

## Launch documents

`launch/` holds the launch records ([index](launch/README.md)). They began
as a snapshot of a local launch workspace and are maintained here. Records
marked historical keep their September 2026 content. They and the gate data
name evidence under `assessment/`, `batch-*/`, `decision-register/`,
`evidence/` and `snapshots/`: local archives (about 9 GB of source
snapshots, logs and binaries) not included here.

Start with:

- [launch/GENESIS_FRAMEWORK.md](launch/GENESIS_FRAMEWORK.md): what genesis contains and the order of work
- [launch/TRUST_MODEL.md](launch/TRUST_MODEL.md): what you trust at launch
- [launch/MAINNET_GATE_MASTER.md](launch/MAINNET_GATE_MASTER.md): the 35 launch gates
- [launch/DECISIONS_REQUIRED.md](launch/DECISIONS_REQUIRED.md): decisions and required records
- [launch/E05_INTAKE.md](launch/E05_INTAKE.md): production values and records
- [launch/MAINNET_V1_SPEC.md](launch/MAINNET_V1_SPEC.md), [launch/DGT_TOKENOMICS.md](launch/DGT_TOKENOMICS.md), [launch/DRT_TOKENOMICS.md](launch/DRT_TOKENOMICS.md): specification and token models
- [launch/USER_LAUNCH_REQUIREMENTS.txt](launch/USER_LAUNCH_REQUIREMENTS.txt): the original launch requirements

## Provenance

[PROVENANCE.json](PROVENANCE.json) records the source path, branch, commit and
a content digest for each folder. The sources were local repositories restored
from snapshots on 2026-09-09. The import includes their uncommitted work as of
2026-09-26.

## Keys and secrets

This tree contains no private keys. The ML-DSA files under
`node/crates/runtime-crypto/tests/fixtures/` are public test vectors. Never
commit validator, custody, faucet or wallet secrets here. Keys that have ever
appeared elsewhere in this repository (for example `pqc_keys.json` or
`testnet/init/pqc_keys/`) are compromised and must not be used for mainnet.
