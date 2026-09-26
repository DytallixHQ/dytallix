# State model v2

Status: proposal. Scope: storage, history verification and state commitment
in the consensus application (`dytallix-fast-launch/node`). This changes the
application hash and storage layout, so it must land before genesis.

Line references are to `dytallix-fast-launch/node/src/consensus_settlement.rs`
unless another file is named.

## Problems

### P1. Hard height cap

`MAX_HISTORY = 100_000` (line 63) is enforced for block inputs (`input_limits`,
line 1918), for committed history (`verify_recovery`), and for the handover
and upgrade histories (lines 2385, 2541). The chain stops accepting blocks at
height 100,000: about 28 hours at the fixture's 1-second commit delay, or
about six days at 5 seconds.

### P2. Full-history re-verification on every call

`verify_recovery` (line 4837) replays blocks 1..H on every call. For each
block it re-derives the input and result digests, checks parent and app-hash
linkage, re-verifies the ML-DSA signature of every committed ordinary and
recovery transaction, re-checks historical validator proofs, receipts and
validator-set transitions, and then scans the whole database twice (the
state digest and the unknown-key check). It also replays the emergency,
upgrade and handover control histories.

It is called from `info` (3287), several `check_tx` paths (3346, 3414, 3509,
3539), admission eviction and counting (3579, 3661), `prepare_proposal`
(3711), `process_proposal` (4100), `finalize_block` (4109) and `commit`
(4543). Each call costs O(all history), including signature verification of
every transaction ever committed. Because CheckTx is unauthenticated input,
this is also a denial-of-service amplifier.

### P3. Full-scan state commitment

`state_digest` (line 872) iterates the entire database, keeps keys under the
`selected` prefixes (line 839), serializes all of them to one JSON value and
hashes it with SHA-256. Every block therefore costs O(state size). The
result supports no inclusion proofs, so there are no light clients and no
verifiable state sync.

### P4. Lifetime activity caps

- Ordinary transactions: receipts are retained inside the single
  `ordinary:v1:state` value, bounded by `max_receipts <= 65,536`
  (`ordinary_state.rs:26`, `ordinary_execution.rs:536`). When the bound is
  reached, every further ordinary transaction is rejected. A test asserts
  this (`receipt_retention_limit_rejects_next_nonce_before_fee_acceptance`).
- Recovery sponsorships: `MAX_RECEIPTS = 65,536` with "all sponsorship
  history is retained" (`recovery_fees.rs:18, 109, 238`).

Together these cap total chain usage for its whole life, independent of the
height cap.

### P5. Monolithic module state

Ordinary, recovery, governance, lifecycle, penalty, emergency, upgrade and
handover state are each stored as one serialized value (for example
`ordinary:v1:state`, `recovery:v1:book`, `governance:v1:state`). Every change
rewrites the whole value, whose size grows with retained history.

### P6. Writes cannot delete

`Writes` is `BTreeMap<Vec<u8>, Vec<u8>>` (`block_lifecycle.rs:11`). A block can
only insert or overwrite keys, so consensus state can never shrink.

### P7. Duplicate block store

Each height stores a full `BlockRecord` (input transactions, results, head,
accepted records) under `consensus:v1:block:`. This duplicates CometBFT's block
store. Today it exists to feed the replay in P2.

### P8. Issuance epoch cap and per-block scans

Adaptive issuance stops permanently once the epoch index exceeds
`max_recorded_epochs` ("Issuance journal audit limit reached",
`runtime/issuance_timing.rs`), configurable up to 1,000,000. Every block also
re-verified the whole issuance journal and scanned the entire database to
build its view. The supply check (`supply::validate_native`, called in every
`prepare`) scanned the entire database as well.

### P9. Per-block emission events in consensus state

`emission:event:{height}` is written every block (`block_lifecycle.rs`) under
the `emission:` state prefix. Consensus only reads the current and previous
events, but the state digest and supply view read one more entry for each past
block.

### P10. Emergency history walk reachable from CheckTx

CheckTx for an upgrade control called `upgrade_plan`, which rebuilt the
emergency history by decoding every committed block before the control's
signature was checked. Any well-formed forged control cost a full walk.

### P11. Validator-set history cap

Lifecycle `history` and `update_history` grow by one entry per validator-set
change, are never pruned, and must stay at or below 10,000 entries
(`runtime/validator_lifecycle.rs`). After 10,000 changes the lifecycle state
fails validation and the chain stops.

### P12. Recovery book: 4,096 accounts in one value

`recovery:v1:book` holds every recovery account, sponsor receipt and operation
index in one value of up to 64 MiB, rewritten every block because its height
advances (`recovery_fees.rs`). `MAX_ACCOUNTS = 4096`, and ordinary
transactions require a recovery account, so the ordinary account system is
capped at 4,096 accounts. Sponsor receipts are capped at 65,536 and must cover
every sponsor nonce ("No pruning is authorized"; "All sponsorship history is
retained").

### P13. Unbonding records retained forever

The validator lifecycle keeps every unbonding record and requires their count
to equal `next_unbond_id` (`runtime/validator_lifecycle.rs`). With
`MAX_ITEMS = 10,000`, the chain stops after 10,000 unbonds in total. Each
record also requires the validator-set view at its exposure height, which ties
unbond retention to validator-set history (P11).

## Invariants to preserve

`verify_recovery` enforces real invariants. The redesign keeps each of them,
checked once at the point where it can change rather than on every call:

1. Genesis source binding, monetary genesis marker and chain ID.
2. Native supply validation and reward-validator consistency.
3. Height counters agree (consensus head, issuance timing, emission,
   governance, recovery, ordinary, penalty).
4. Block linkage: parent engine hash, prior application hash, monotonic time,
   input and result digests, one result per input transaction.
5. Validator updates match lifecycle history and activate at H+2.
6. Epoch observations appear only at epoch boundaries, at index 0.
7. Every receipt matches its authenticated input; no orphan receipts,
   transactions or duplicate positions.
8. Penalty incidents match committed evidence.
9. No unknown or unconfigured state keys.
10. Emergency, upgrade and handover histories are consistent.

## Design

### D1. Verify once, then extend incrementally

- Keep the current full check as `verify_full`, run at startup and by an
  offline verification command. Remove it from every per-call path.
- At commit, verify only the new block against the committed head held in
  memory: invariants 3–7 for that block, O(block).
- Persist derived control-plane state (the emergency trace, upgrade outcomes
  and handover state) and update it at commit, instead of recomputing it from
  history.
- Startup always runs an O(1) consistency check (head, height counters and
  state root agree). The full check becomes a flag and an operator command.
- Commit is already one atomic RocksDB write batch, so no partial block can be
  observed after a crash.

Result: CheckTx, proposal and commit cost depends on the block, not on chain
age.

### D2. Remove the height cap

- Delete `MAX_HISTORY`. Heights are `u64`.
- Where history is still needed (evidence age, control windows), use explicit
  retention windows derived from configuration, such as
  `evidence_max_age_blocks` plus a margin.
- Retain `BlockRecord`s only within that window, then prune them. CometBFT
  remains the canonical block store.

### D3. Authenticated state commitment

| Option | Summary | Assessment |
| --- | --- | --- |
| Jellyfish Merkle Tree (`jmt` crate) | Versioned sparse Merkle tree over hashed keys; used by Aptos, Penumbra and Sovereign | O(log n) updates and proofs, inclusion and non-inclusion proofs, export by version for state sync. Needs a dependency audit |
| In-house sparse Merkle tree | About 600 lines, SHA3-256 | Smallest dependency surface; more code to review |
| IAVL | Cosmos tree | Go only; historically slow |
| Merkle Patricia Trie | Ethereum tree | Complex; large proofs |

**Recommendation:** JMT with a SHA3-256 hasher, consistent with
`dytallix_protocol_types::sha3_256`. Accept it only if its dependency graph
passes `scripts/check_consensus_cargo_profile.py` (no prohibited classical
cryptography). Otherwise use an in-house binary sparse Merkle tree. `jmt` is
not in the local crate cache, so the audit needs a network fetch.

- Application hash v2 = SHA3-256(`dytallix-app-v2` ‖ state root ‖ anchor
  digest). The anchor keeps parent linkage.
- Only consensus state keys enter the tree (the current `selected` prefixes).
  Indexes (`tx:`, `rcpt:`, block records) stay outside it; they are not
  consensus-critical and can be pruned.
- `Writes` becomes `BTreeMap<Vec<u8>, Option<Vec<u8>>>`, where `None` deletes a
  key (fixes P6).

### D4. Decompose module state

Store one key per entity: ordinary accounts, nonces and receipts; recovery
accounts and sponsor receipts; governance proposals and votes; lifecycle
validators; penalty incidents. A block then writes O(changed entities), and
each entity can be proven individually under D3.

### D5. Replace lifetime receipt retention with an expiry window

Ordinary transactions already carry a strictly increasing `spending_nonce` and
an `expiry_height`, bounded by the fee profile's `max_expiry_lifetime`. A
receipt only needs to be kept until its transaction's expiry height has
passed; after that, the nonce alone prevents replay. Prune receipts whose
expiry height is below the current height. Recovery sponsorship uses
`sponsor_nonce` the same way.

This removes both 65,536 lifetime caps. The retained set is then bounded by
throughput multiplied by the expiry window.

### D6. Snapshots and state sync

Implement ABCI snapshots over JMT version export: list, offer, load and apply
chunks, each verified against the application hash at the snapshot height.
The bridge currently stubs these methods
(`consensus/cometbft/cmd/dytallix-comet-bridge/application.go`). This lets new
validators join without replaying from genesis.

## Phases

| Phase | Content | Exit tests |
| --- | --- | --- |
| A1 (done) | D1: verification mark, per-block commit check, prefix state digest | Zero complete checks during normal operation; one after an outside write; digest reads only state keys |
| A2 (done) | D2 cap removal; prefix reads for the issuance and supply views; emergency records from stored receipts, proven equal to history by the complete check (P8 scans, P10) | Block inputs accepted beyond 100,000; existing suite green |
| A3 (done) | Ordinary receipts kept only for the fee profile's maximum transaction lifetime (D5, P4 ordinary); the complete check accepts pruned heights. Supply validation reads only the current emission event (P9, supply side) | Capacity returns after the window; pruned transactions cannot replay; reopen passes the complete check; supply valid with past events deleted |

Deferred from A3, because each needs a redesign rather than a window:

| Item | Moves to | Reason |
| --- | --- | --- |
| Recovery book and sponsor receipts (P12, P4 recovery) | Phase B, D4 | Needs per-account keys and nonce-plus-expiry replay protection instead of full sponsor history |
| Validator-set history and unbonding records (P11, P13) | Step 4 (stake withdrawals) | Both rework the lifecycle module, whose history entries cross-validate each other |
| Issuance journal and epoch cap (P8) | Phase B, with deletions | Needs a controller checkpoint plus pruning of per-epoch records. Until then, `max_recorded_epochs` (up to 1,000,000) and the epoch length bound it; with daily epochs the per-block replay grows by about 365 controller steps a year. Step 3 made the observations deterministic but left the journal unchanged |
| Emission events in the state commitment (P9, digest side) | Phase B, D3 | An incremental Merkle root removes the per-block read of all state keys |
| B | D4, D3, deletions (app hash v2) | Tree root equals a naive reference over the same key set (property tests); inclusion and non-inclusion proofs verify; determinism across independent databases; supply invariants; G35 profile PASS |
| C | D6, block-record pruning | A new node joins from a snapshot and matches the application hash; pruned nodes keep full consensus correctness |

Each phase keeps the node test suite green and the G35 dependency check at
PASS.

## Compatibility

No mainnet exists, so the new application hash and layout need no migration.
Development fixtures and vectors are regenerated. The upgrade registry's only
migration, `emergency-receipt-digest-index-v1`, targets the old layout and
must be revisited in phase B.

## Gates

Directly: G01 and G02 (consensus stability and safety), G09 (state recovery),
G25 (RPC stability), G26 (load). Indirectly: G24 (upgrades), because the
storage layout must be stable before the first upgrade path is qualified.

## Open questions

1. Target block time. It sets the retention windows and the load targets.
2. Whether wallets and the explorer need proof-backed queries at launch. That
   decides whether phase B precedes the public candidate testnet.
3. The outcome of the `jmt` dependency audit.
