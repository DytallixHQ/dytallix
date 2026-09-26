# Node Operators

This page summarizes the published Dytallix node snapshot, the reproducible
deployment path published with it, and the practical defaults that matter when
running it locally.

Keypair, faucet, transfer, and basic contract lifecycle are available for experimentation on the public testnet. Staking, governance, and some advanced or operator paths are not yet production-complete.

## Published Node Snapshot

The local source snapshot used for this page was the published tree rooted at:

```text
dytallix-node-publish
```

Primary components:

- `dytallix-fast-launch/node` — public RPC node and execution engine
- `blockchain-core` — shared chain logic
- `pqc-crypto` — PQC helpers and CLIs
- `smart-contracts` — contract runtime and examples

## Build

```bash
cd dytallix-fast-launch/node
cargo build --release --locked
```

For the clean checkout deployment templates and install flow, also see
`docs/build-and-run.md` in the node repo.

## Runtime Defaults Observed In Source

The published `main.rs` currently defaults to:

- HTTP bind port: `3030`
- data dir: `./data`
- chain ID: `dyt-local-1`
- block interval: `15000` ms
- empty blocks: enabled
- max transactions per block: `100`
- websocket support: enabled

Important note:

- the older `README_RPC.md` still says the block interval default is `2000` ms
- the source code currently says `15000` ms

If you are operating from source, use the code default.

## Core Environment Variables

- `DYT_DATA_DIR` — node data directory
- `DYT_CHAIN_ID` — persisted chain ID
- `DYT_BLOCK_INTERVAL_MS` — background block production interval
- `DYT_EMPTY_BLOCKS` — whether empty blocks are produced
- `BLOCK_MAX_TX` — per-block transaction cap
- `DYT_WS_ENABLED` — websocket toggle
- `RUNTIME_MOCKS` — relaxed development mode
- `FRONTEND_ORIGIN` — CORS origin override
- `MAX_TX_BODY` — submit-body size cap
- Genesis: the inspected fast-node startup reads `./genesis.json` from its working directory. `DYT_GENESIS_FILE` does not select this startup file.

Feature toggles present in source:

- `DYT_ENABLE_GOVERNANCE`
- `DYT_ENABLE_STAKING`

## Storage

The node persists chain data under RocksDB. The published docs and source show
storage for:

- account state
- blocks
- transactions
- receipts
- chain metadata

The node also persists chain ID to prevent accidental local fork or data-dir
reuse under a different network identifier.

## Local RPC

The published node source exposes routes such as:

- `/status`
- `/health`
- `/account/:addr`
- `/balance/:addr`
- `/block/:id`
- `/blocks`
- `/tx/:hash`
- `/api/blockchain/submit`
- websocket route `/ws`

## Secrets And Validator Keys

The published node documentation explicitly avoids plaintext validator key
persistence.

Supported patterns:

- Vault KV v2
- sealed local keystore

Relevant environment variables include:

- `DYTALLIX_VAULT_URL`
- `DYTALLIX_VAULT_TOKEN`
- `DYTALLIX_VAULT_KV_MOUNT`
- `DYTALLIX_VAULT_PATH_BASE`
- `DYT_KEYSTORE_DIR`
- `DYT_KEYSTORE_PASSPHRASE`
- `VALIDATOR_ID`

## Fee And Denom Behavior

The current node source and live `/status` endpoint both report fees in
`udgt`.

That means:

- the public testnet is presently charging fees in the governance-token denom
- this overrides older nearby docs that described `DRT` as the fee token

## Local Tooling Alignment

The current local SDK and CLI defaults now align with the node snapshot:

- local node port: `3030`

Use `3030` as the canonical local node port for direct integrations and CLI
workflows.

## Public Deployment Provenance

The current public node behavior can be checked from public endpoints such as
`/status` and `/api/capabilities`, and the repo now includes clean `systemd`
and `pm2` deployment templates.

That is enough to publish a reproducible deployment path from source, but not
enough on its own to prove that the live production host has already been cut
over to a clean checkout. Independent operator-side evidence is still required
before the node should be described as a fully proven canonical deployment repo.

## Operational Guidance

- treat the published node snapshot as the source of truth for local runtime
  defaults
- treat the public gateway as the source of truth for public integration
  behavior
- verify the gas schedule from `/status` before building fee assumptions into
  automation
