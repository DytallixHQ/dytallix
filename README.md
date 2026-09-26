# Dytallix mainnet

This folder holds the Dytallix mainnet candidate. It is kept separate from the
testnet and product code in the rest of this repository.

**Status: NO GO.** Mainnet is not launched and no launch is authorized. See
[launch/MAINNET_READINESS_REPORT.md](launch/MAINNET_READINESS_REPORT.md) and
[launch/LAUNCH_GATES.json](launch/LAUNCH_GATES.json) for the 35 launch gates.

## Layout

| Folder | Contents |
|---|---|
| [node/](node/) | Rust node workspace (`dytallix-fast-node`), CometBFT v0.40.0 fork with PQC transport (`node/consensus/cometbft`), PQC HTTP adapter, native supervisor, adaptive emission, storage, gas and signature-policy crates |
| [sdk/](sdk/) | Rust SDK and `dytallix` CLI, including the ordinary-v2 client and browser crate |
| [pqc/](pqc/) | PQC primitives (ML-DSA, SLH-DSA, ML-KEM, FN-DSA). The node is the qualification authority. |
| [contracts/](contracts/) | WASM reference contracts: DGT, DRT, emission, staking, governance, algorithm registry |
| [faucet/](faucet/) | Testnet faucet service |
| [docs/](docs/) | Public documentation source (MkDocs). Written for testnet; needs a mainnet revision. |
| [launch/](launch/) | Mainnet specification, tokenomics, genesis drafts, launch gates, decision register |

## Build

Toolchains: Rust 1.88.0 (pinned by `rust-toolchain.toml`) and Go 1.25.

```sh
cd mainnet/node
cargo build --workspace --all-targets --locked
cargo test --workspace --locked
```

The default build is the mainnet consensus application (feature
`pqc-consensus`). The archived legacy node that the current testnet runs is a
separate, explicit build; see "Build modes" in `node/docs/build-and-run.md`.

The consensus engine lives in `node/consensus/cometbft`:

```sh
cd mainnet/node/consensus/cometbft
go test -mod=readonly ./...
```

See `node/consensus/cometbft/README.md` and `PQC_ENGINE_INTEGRATION.md` there.

CI runs all of the above on every change under `mainnet/`
(`.github/workflows/mainnet.yml`).

## Launch documents

`launch/` is a snapshot of the top-level documents from the local launch
tracking workspace. Some of them link into `decision-register/`, `batch-*/` and
`evidence/`. Those directories are local evidence archives (about 9 GB of
source snapshots, logs and binaries) and are not included here.

Start with:

- [launch/USER_LAUNCH_REQUIREMENTS.txt](launch/USER_LAUNCH_REQUIREMENTS.txt): launch requirements
- [launch/MAINNET_V1_SPEC.md](launch/MAINNET_V1_SPEC.md): protocol specification (draft)
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
