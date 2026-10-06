# Dytallix mainnet

This is the Dytallix mainnet candidate. It began as the `mainnet/` folder of
exocognosis/dytallix, separate from the testnet and product code there, and
moved here with its history on 5 October 2026
([release/MOVE.md](release/MOVE.md)).

**Status: NO GO.** Mainnet is not launched and no launch is authorized. See
[launch/MAINNET_READINESS_REPORT.md](launch/MAINNET_READINESS_REPORT.md) and
[launch/LAUNCH_GATES.json](launch/LAUNCH_GATES.json) for the 35 launch gates.

## Layout

| Folder | Contents |
|---|---|
| [node/](node/) | Consensus application (`dytallix-fast-node`), CometBFT v0.40.0 fork with PQC transport (`node/consensus/cometbft`), PQC HTTP adapter, native supervisor, adaptive emission and storage crates |
| [sdk/](sdk/) | Rust SDK and `dytallix` CLI, including the ordinary-v2 client and browser crate |
| [pqc/](pqc/) | PQC primitives (ML-DSA, SLH-DSA, ML-KEM, FN-DSA). The node is the qualification authority. |
| [contracts/](contracts/) | WASM reference contracts: DGT, DRT, emission, staking, governance, algorithm registry. Not part of the consensus build; nothing here runs on the chain. |
| [docs/](docs/) | Public documentation source (MkDocs). Mostly written for the public testnet; the tokenomics, security model, CLI reference, contract quickstart and whitepaper errata pages describe the mainnet candidate. |
| [launch/](launch/) | Mainnet specification, tokenomics, genesis drafts, launch gates, decision register |
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

Toolchains: Rust 1.88.0 (pinned by `rust-toolchain.toml`) and Go 1.25.

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

CI runs all of the above on every change (`.github/workflows/mainnet.yml`),
and [release/README.md](release/README.md) describes the reproducible release
build.

## Contributing and license

See [CONTRIBUTING.md](CONTRIBUTING.md): outside contributions are signed off
under the [Developer Certificate of Origin](DCO). Report vulnerabilities
privately ([SECURITY.md](SECURITY.md)). Dytallix is dual licensed under the
[MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE),
at your option; third-party code keeps its own license.

## Launch documents

`launch/` is a snapshot of the top-level documents from the local launch
tracking workspace. Some of them link into `assessment/`, `batch-*/`,
`decision-register/`, `evidence/` and `snapshots/`. Those directories are local
evidence archives (about 9 GB of source snapshots, logs and binaries) and are not
included here.

Start with:

- [launch/USER_LAUNCH_REQUIREMENTS.txt](launch/USER_LAUNCH_REQUIREMENTS.txt): launch requirements
- [launch/MAINNET_V1_SPEC.md](launch/MAINNET_V1_SPEC.md): protocol specification (draft, stale in part; see its notice)
- [launch/DECISIONS_REQUIRED.md](launch/DECISIONS_REQUIRED.md): open decisions
- [launch/DGT_TOKENOMICS.md](launch/DGT_TOKENOMICS.md), [launch/DRT_TOKENOMICS.md](launch/DRT_TOKENOMICS.md): token models
- [launch/GENESIS_SPEC.md](launch/GENESIS_SPEC.md): genesis requirements

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
