# Repository Map

[Docs hub](README.md) | [Build and run](build-and-run.md)

## Cargo Workspace

| Path | Cargo package | Purpose |
| --- | --- | --- |
| [`dytallix-fast-launch/node`](../dytallix-fast-launch/node) | `dytallix-fast-node` | Consensus application |
| [`crates/adaptive-emission`](../crates/adaptive-emission) | `dytallix-adaptive-emission` | DRT adaptive emission controller |
| [`crates/gas`](../crates/gas) | `dytallix-gas` | Gas and fee metering |
| [`crates/native-supervisor`](../crates/native-supervisor) | `dytallix-native-supervisor` | Supervisor that hosts the consensus application |
| [`crates/protocol-types`](../crates/protocol-types) | `dytallix-protocol-types` | Wire types, addresses and canonical encodings |
| [`crates/release-runtime`](../crates/release-runtime) | `dytallix-release-runtime` | Release ownership and observation runtime |
| [`crates/runtime-crypto`](../crates/runtime-crypto) | `dytallix-runtime-crypto` | FIPS 204 ML-DSA-65 signing and verification |
| [`crates/signature-policy`](../crates/signature-policy) | `dytallix-signature-policy` | Signature policy |
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

- `consensus_stdio`: pipe protocol driven by the CometBFT bridge
- `lifecycle_fixture`: validator lifecycle fixture
- `pqc_signer`, `txhash`: signing and hashing utilities
- `helper-execution-qualification`: requires feature `helper-qualification`

Notable source areas in the application:

- [`consensus_settlement.rs`](../dytallix-fast-launch/node/src/consensus_settlement.rs): ABCI handlers and block settlement
- [`runtime/`](../dytallix-fast-launch/node/src/runtime): staking, rewards, issuance, penalties and governance
- [`mempool/`](../dytallix-fast-launch/node/src/mempool): admission
- [`storage/`](../dytallix-fast-launch/node/src/storage), [`state/`](../dytallix-fast-launch/node/src/state): state access
- [`upgrade/`](../dytallix-fast-launch/node/src/upgrade): upgrade migrations
