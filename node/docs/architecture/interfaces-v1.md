# Interfaces of the consensus chain (E04 gap 9)

Status: approved (P01, 28 September 2026: decisions below). Engineering
task E04, gap 9 of the [E04.1 triage](../mainnet/e04-requirement-triage.md)
(API-001). Paths: `sdk/` is `mainnet/sdk`, `node/` is `mainnet/node`.

The inventory is [`interfaces-v1.json`](interfaces-v1.json). Every external,
operator-facing and internal interface of the candidate is listed there
with its exposure, format, version, entry point and tests.
`scripts/check_interface_inventory.py` checks it against the source: each
file and symbol exists, each version constant has the recorded value, and
each named test exists. A node test checks that the inventory's ABCI query
paths are exactly the ones the application accepts.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| I1 | No inventory of interfaces; nothing ties documentation to the code. | — |
| I2 | Unversioned surfaces: the `/status` view (also `""` and `/supply`), the `/emergency/receipt` view, the metrics files, the light-block export output, the engine's ready line, the ABCI info label (`batch9`), the keystore, the CLI configuration and pinned-chain files, and every CLI JSON output. | various |
| I3 | Stake, rewards and governance are readable only as raw internal records; the reward and validator state are single records of up to 16 MiB. | `node/.../runtime/reward_runtime.rs`, `validator_lifecycle.rs`, `governance_store.rs` |
| I4 | No ABCI events, and no statement whether there should be. | `node/consensus/cometbft/cmd/dytallix-comet-bridge/application.go` |
| I5 | The CLI keystore holds private keys in plaintext. | `sdk/crates/dytallix-sdk/src/keystore.rs` |

## Decisions (P01, approved 28 September 2026)

1. **Versioning.** Paths stay as they are. Every view, file and CLI JSON
   output carries an explicit version, recorded in the checked inventory.
   An incompatible change gets a new path, file version or output version;
   a compatible addition keeps the version.
2. **Typed reads.** New versioned query views, bound to the committed head
   like `/ordinary/account`: an account summary (liquid, bonded, unbonding,
   claimable rewards), the validator set, and governance proposals and
   votes. They are the node's report, not proofs; balances stay provable
   (clients v1, K-d).
3. **Events.** No ABCI events in v1. This fork's results hash excludes
   events (`DeterministicExecTxResult` keeps code, data and gas), so they
   would be unauthenticated. Receipts and blocks are the committed
   per-transaction record; explorers and wallet history use them and a
   block-scanning indexer. Events can come later without a chain upgrade.
4. **Keystore.** Encryption at rest is a new E04 gap (16): a
   passphrase-derived key with an AEAD cipher, a versioned file format, and
   migration from the plaintext format. Gap 9 records the plaintext format
   as version 1 and flags it.

## Rules

- **Views.** Each query view has a top-level `version`. Clients refuse a
  version they do not know (the SDK already does for its views).
- **Transports.** Each transaction format carries its version in its bytes
  or discriminator (ordinary `VERSION` 2 and 3, recovery wire 1, control
  `kind` strings with `-v1`/`-v2`).
- **Results.** Transaction results are CometBFT's fixed fields (code, log,
  gas). Their meaning (codes 0 success, 1 refused and not charged, 2
  infrastructure failure, 3 charged failure) belongs to the application
  protocol version, `ResponseInfo.app_version`. `ResponseInfo.version`
  names the interface generation, `dytallix-app-v1`.
- **Files.** The keystore, CLI configuration and pinned-chain files carry
  `version`. A file without one reads as version 1, the format before this
  rule; writers always write it.
- **Operator outputs.** The metrics files expose
  `dytallix_metrics_format_version{process}`; the light-block export,
  snapshot metadata and engine ready line carry `version` or `format`.
- **CLI output.** Every JSON object the CLI prints, including errors, has
  `output_version`.
- **Internal channels** (the stdio bridge, the supervisor protocol) are
  versioned by the release manifest that pins both sides; they are listed
  but not public.

## I-a implementation notes

- **Inventory.** `interfaces-v1.json` lists 36 interfaces in ten groups
  (ABCI queries, transactions, results, engine RPC, P2P, operator outputs,
  internal channels, SDK, CLI, files). Each entry names its exposure,
  format, version, entry point and tests. `scripts/check_interface_inventory.py`
  (with its unit tests, in the node CI job) checks it against the source;
  `inventory_lists_exactly_the_accepted_query_paths` checks that the ABCI
  query entries are exactly what `query_path` accepts.
- **New versions.** `/status` (`STATUS_VIEW_VERSION`) and
  `/emergency/receipt` (`EMERGENCY_RECEIPT_VIEW_VERSION`) views; both
  metrics files (`dytallix_metrics_format_version`); the light-block export
  summary; the engine ready line; the keystore, CLI configuration and
  pinned-chain files (files without a version read as version 1); and every
  CLI JSON object (`output_version`).
- **ABCI info.** `ResponseInfo.version` is `dytallix-app-v1` and `data` is
  `Dytallix consensus application`, replacing the `batch9` placeholder.
  The supervisor's readiness check requires the new labels.
- **Keystore permissions.** The keystore was written with default
  permissions, usually world-readable, and the consensus commands, which
  require an owner-only key file, refused it. It is now written owner-only
  (0600) through a temporary file and a rename.

## Steps

| Step | Content |
| --- | --- |
| I-a | Inventory and its check; version fields where missing; gap 16 recorded |
| I-b | Typed query views: account summary, validator set, governance proposals and votes; SDK and CLI reads |
