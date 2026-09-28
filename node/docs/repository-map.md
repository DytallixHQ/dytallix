# Repository Map

[Docs hub](README.md) | [Build and run](build-and-run.md)

## Cargo Workspace

| Path | Cargo package | Purpose |
| --- | --- | --- |
| [`dytallix-fast-launch/node`](../dytallix-fast-launch/node) | `dytallix-fast-node` | Consensus application |
| [`crates/adaptive-emission`](../crates/adaptive-emission) | `dytallix-adaptive-emission` | DRT adaptive emission controller |
| [`crates/native-supervisor`](../crates/native-supervisor) | `dytallix-native-supervisor` | Supervisor that hosts the consensus application |
| [`crates/protocol-types`](../crates/protocol-types) | `dytallix-protocol-types` | Wire types, addresses and canonical encodings |
| [`crates/release-runtime`](../crates/release-runtime) | `dytallix-release-runtime` | Release ownership and observation runtime |
| [`crates/runtime-crypto`](../crates/runtime-crypto) | `dytallix-runtime-crypto` | FIPS 204 ML-DSA-65 signing and verification |
| [`crates/storage`](../crates/storage) | `dytallix-storage` | RocksDB state, blocks, receipts and transaction records |

[`consensus/pqc-http-adapter`](../consensus/pqc-http-adapter) is a separate
Cargo workspace.

## Go Modules

| Path | Module | Purpose |
| --- | --- | --- |
| [`consensus/cometbft`](../consensus/cometbft) | `dytallix.local/consensus/cometbft` | Engine, ABCI bridge and qualification tools; the fork lives in `upstream/` |
| [`consensus/owner-guard`](../consensus/owner-guard) | `dytallix.local/consensus/owner-guard` | Process-ownership guard |
| [`consensus/root-authorization`](../consensus/root-authorization) | `github.com/dytallix/root-authorization` | Root authorization verifier |

## Consensus Application Binaries

In [`dytallix-fast-launch/node/src/bin`](../dytallix-fast-launch/node/src/bin):

- `consensus_stdio`: pipe protocol driven by the CometBFT bridge; feature
  `test-snapshot-verifier` builds the test application for the signed
  process tests, which the bridge refuses
- `pqc_signer`: signing utility
- `dytallix-state-check`: read-only startup checks of a stopped node's
  database, for operators (E04 gap 15)
- `helper-execution-qualification`: requires feature `helper-qualification`

Notable source areas in the application:

- [`consensus_settlement.rs`](../dytallix-fast-launch/node/src/consensus_settlement.rs): ABCI handlers and block settlement
- [`runtime/`](../dytallix-fast-launch/node/src/runtime): staking, rewards, issuance, penalties and governance
- [`storage/`](../dytallix-fast-launch/node/src/storage), [`state/`](../dytallix-fast-launch/node/src/state): state access
- [`upgrade/`](../dytallix-fast-launch/node/src/upgrade): upgrade migrations
