# State sync and bounded restart (state model phase C)

Status: approved (P01, 27 September 2026: decisions below); C1 to C5
implemented (notes below). Engineering task E04. Covers gaps 3 and 4 of the
[E04.1 triage](../mainnet/e04-requirement-triage.md) (STATE-002, STATE-003,
SYNC-002, SYNC-003) and D6 of [state model v2](state-model-v2.md). Paths:
`src/` is `dytallix-fast-launch/node/src`, `cometbft/` is
`consensus/cometbft`.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| S1 | Every restart runs the complete check from block 1: it replays every block record and re-verifies every recovery and ordinary ML-DSA signature. The verification mark lives in memory only. Startup time grows with the chain. | `src/consensus_settlement.rs` (`verify_recovery`, `verify_history`) |
| S2 | Block records (`consensus:v1:block:`), transaction indexes and one `emission:event:` per block are never pruned. The engine is always told `RetainHeight: 0`, so its block store is never pruned either. | `cometbft/cmd/dytallix-comet-bridge/application.go:259` |
| S3 | A new node can only replay from genesis. The bridge's snapshot methods are stubs; two engine gates reject state sync; the only light-client state provider uses HTTP and is excluded from the PQC build (G35). | `application.go:295-306`, `enginepqc/engine.go:123`, `upstream/node/services_pqc.go:13`, `upstream/statesync/stateprovider_pqc.go` |
| S4 | A restored node could not pass its own startup check (it needs blocks from 1 and the genesis source) and could not derive the next epoch observation (it reads the previous epoch's block records). | `verify_history`, `derived_observation` |
| S5 | One lock serializes every bridge call into the application, so serving a snapshot chunk to a peer would delay block finalization. | `dytallix-comet-bridge/main.go:87-91` |

What already works: validators answer snapshot requests through the engine's
state-sync reactor; light-client header verification uses the same ML-DSA-65
path as consensus; the state root (phase B) authenticates every committed
key; each block record's anchor carries the prior application hash, so
records chain back from any trusted head.

## Proposed rules

1. **Startup check (fixes S1).** At startup the application checks the
   current state completely (the state tree rebuilt against the head, and
   every module's invariants on current state: supply, accounts, lifecycle,
   penalties, governance, issuance window, fees), then replays only the
   blocks it still holds, back to the start of its retained window, checking
   the anchor chain from the head. Nothing is replayed from genesis.
2. **Retention (fixes S2).** A node keeps block records for a window derived
   from existing limits, with no new parameter: the longest of the evidence
   horizon (evidence age plus margins), two issuance epochs, and the
   transaction receipt window (recovery receipts: see the C1 notes). Older
   block records are removed at commit, and the engine is told the same
   retain height. `emission:event:` records older than the parent are
   removed from state. An operator may run an archive node that keeps all
   block records (a local setting; it changes no consensus state).
3. **Snapshots (fixes S3, S5).** Every N blocks (an E05 value) the
   application writes a snapshot of height H to files: the committed state
   entries in sorted chunks of at most 4 MiB, plus the retained block records
   and head. Metadata lists each chunk's SHA3-256 and the state root. The
   bridge serves listed snapshots and chunks from those files directly, so
   serving never takes the application lock. The last K snapshots are kept
   (E05).
4. **Restore.** A joining node accepts a snapshot only for the height and
   application hash its light client trusts. Each chunk must match its
   listed hash; entries go to a staging database. After the last chunk the
   tree is rebuilt, the root with the head's anchor must give the trusted
   application hash, retained block records must chain back from the head,
   and the startup check must pass. Only then does the staging database
   replace the live one; an interrupted restore is discarded.
5. **Trust.** The engine verifies headers with ML-DSA-65 from a trusted
   height and hash that the operator configures, within a trust period below
   the unbonding and evidence horizon. The operator supplies the needed light
   blocks (headers, commits and validator sets), exported from a node they
   run, to the joining node locally; its light client verifies every
   signature, so the export is trusted for availability only. No new network
   protocol.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| C1 | Startup check from the retained window (rule 1); retention and retain height (rule 2) | Restart cost; storage bounded |
| C2 | Snapshot writer, file layout, bridge serving (rule 3) | New files |
| C3 | Restore path in the application and bridge (rule 4) | State sync possible |
| C4 | Engine: lift the two gates for the configured profile; header source (rule 5) | Joining node |
| C5 | Tests: restart cost is independent of height; pruned node passes checks; snapshot of a live chain restores on a second database with the same application hash; corrupted chunk, wrong hash, interrupted restore; two-node join through the engine | Tests |

## C1 implementation notes

- **Window start.** The oldest retained record's height is a local key,
  `consensus:v1:retained`, like the records themselves outside the state
  tree. At commit a record leaves when it is below the floor and past the
  evidence horizon at the new head (the lifecycle's own rule for dropping
  validator sets); at most 64 records are examined per commit.
- **Floor.** The longest of: two blocks; two issuance epochs; the emergency
  anchor age bound plus one; the ordinary receipt window
  (`max_expiry_lifetime`, after governed changes).
- **Recovery receipts.** A recovery account sets its own submission
  lifetime, so there is no chain-wide bound to derive a window from. They do
  not extend the window; receipts of removed blocks are checked against
  state only. This narrows rule 2 as approved.
- **Pinned records.** A record a replay reads stays after its block leaves
  the window: one with a recorded emergency, upgrade or handover control,
  one with accepted legacy signed transactions (whose indexes `tx:`, `rcpt:`
  and the replay guard `execution:v1:receipt:` are never removed), and a
  recorded emergency control's finalized anchor. Controls need their
  policy's authority and legacy signed transactions are refused under the
  recovery profile, so these are rare.
- **Startup check.** Replays the blocks after the window start, anchored to
  its record. Evidence naming a removed block is checked for shape; the
  validator fold starts where the lifecycle history holds the set; an epoch
  observation is derived again only when its epoch is held. Receipts,
  penalty incidents and evidence records of removed blocks are checked
  against state. Pinned records below the window are checked for their own
  commitments and the pin rule, and the control replays walk every held
  record.
- **Emission events.** Block H removes `emission:event:{H-2}` from committed
  state. This changes application hashes relative to earlier builds.
- **Engine.** `commit` returns `retain_height` (zero when every record is
  held), and the bridge passes it to CometBFT, which keeps the blocks its
  own evidence checks need.
- **Archive node.** `--block-history archive` on the application command
  keeps every record and reports a retain height of zero; committed state is
  the same (tested).
- **Until C3 and C4.** A node that joins after its peers have pruned cannot
  sync from genesis. Keep an archive node where that matters.

## C2 implementation notes

- **Settings.** `--snapshot-dir`, `--snapshot-interval` and
  `--snapshot-keep` on the application command, all three or none. The
  interval (N) and count kept (K) are E05 values: there are no defaults, and
  without them no snapshot is written.
- **Writer.** When a due height commits, the application takes a RocksDB
  checkpoint while it still holds the execution lock, then a background
  thread writes the files from the checkpoint and removes it. The checkpoint
  uses hard links, so the directory should be on the database's filesystem.
  A snapshot still being written makes the next due height skip. A failure
  is logged and never affects commit.
- **Content.** Every database entry except the state tree's records
  (`merkle:`), which a joining node rebuilds: committed state, block records
  (window and pinned), head, window start and the local metadata the startup
  check reads. Entries are sorted by key and encoded as a 32-bit big-endian
  key length, key, value length and value, as one stream cut into chunks of
  at most 4 MiB. An entry larger than a chunk spans chunks.
- **Files.** `{height:020}/metadata.json` (format 1, chain, height,
  application hash, state digest, window start, entry and byte counts, and
  each chunk's SHA3-256) and `{height:020}/chunk-{index:06}`. A snapshot is
  written under `.staging-{height:020}` and published by renaming it; the
  latest K are kept, and partial directories are removed when the writer
  starts.
- **Serving.** The bridge's `--snapshot-dir` names the same directory.
  `ListSnapshots` offers published snapshots, newest first, with the
  metadata bytes as snapshot metadata and their SHA3-256 as the snapshot
  hash. `LoadSnapshotChunk` returns a listed chunk of at most 4 MiB. Neither
  calls the application. Anything unreadable is not offered: an ABCI error
  would stop the engine's application connection.
- **E02.** The rendered AppArmor profile grants `rwk` under each declared
  writable root, and the service's system call filter allows links and
  renames. A snapshot directory inside the database's writable root needs no
  policy change. The native checks do not yet exercise a snapshot; that
  belongs with the C5 join test.

## C3 implementation notes

- **Offer.** Only a node with no state and no pending block accepts a
  snapshot; one with state answers abort. The format must be 1, the
  metadata canonical with its SHA3-256 equal to the snapshot hash, and its
  height, chunk count, chain and application hash equal to the offer and the
  hash the light client verified. A new offer discards a restore in
  progress.
- **Chunks.** They arrive in order. A chunk that differs from its listed
  hash, or is over 4 MiB, is fetched again from another sender (the bridge
  answers retry, naming the chunk and rejecting its sender). Entries must be
  strictly ascending, outside the state tree, and add up to the listed
  counts. A snapshot is authenticated only once complete, so a false one is
  bounded before then: at most 65,536 chunks, no key or value over 64 MiB.
- **Checks.** Entries go to a staging database at `{db}.restore`. After the
  last chunk the tree is rebuilt at the snapshot height (from an empty root
  at the height before it) and must give the listed state digest; the head
  must give the trusted application hash by the application hash formula;
  and the full startup check must pass: configuration and genesis source,
  root genesis receipt, the complete check over the retained window, the
  control replays with the local verifier, and under handover the active
  release. Any failure rejects the snapshot, removes the staging database
  and leaves the node empty.
- **Swap.** The live handle closes first (RocksDB locks a database by path
  within a process, so a read-only handle to the staging database holds its
  place), the empty live database is renamed to `{db}.replaced`, the staging
  database to `{db}`, and it is reopened; `.replaced` is then removed. At
  open, a leftover `.restore` or `.replaced` is removed: an interrupted
  restore is discarded, and the database it would replace was empty. Both
  sit beside the database, so its parent directory must be in a writable
  root (E02).
- **Limits.** The tree rebuild holds every state key in one update. That is
  fine at current sizes; a streamed rebuild belongs with the T-level scale
  tests. The engine still refuses state sync until C4 lifts its two gates.

## C4 implementation notes

- **Gates.** The engine's isolation check no longer forbids state sync but
  requires the PQC-only build, and upstream `validateBuildServices` no
  longer excludes it. In that build `StateSyncConfig.ValidateBasic` refuses
  `rpc_servers`, since the HTTP light client is not built, and keeps
  upstream's other rules: trusted height, hash and period, chunk timeout and
  chunk fetchers.
- **Light blocks.** `dytallix-pqc-engine start ... --light-blocks DIR`,
  repeated for witnesses. The first export is the primary; the others are
  exports from other nodes the operator runs. A single export is its own
  witness, which adds no cross-check. State sync without an export, or an
  export without state sync, refuses to start, and so does a trust period
  not below the genesis evidence age (`evidence.max_age_duration`).
- **Verification.** CometBFT's light client (`internal/lightblocks`)
  verifies sequentially from the trusted height, so every height from it
  through the snapshot height plus two must be exported. The consensus
  parameters at the snapshot height plus one must hash to that verified
  header's consensus hash. A witness holding a conflicting, validly signed
  header stops verification.
- **Export.** `dytallix-light-export --home HOME --from T --to H+2 --output
  DIR` reads the block and state stores of a stopped node, or a copy of its
  home, and prints the trust height and hash to configure. Each height is
  `{height:020}.block` (a LightBlock protobuf) and `{height:020}.params`.
- **Checks.** E01 records the route as `state_sync_light_client`; the Go
  graph check passes, since the light client adds no HTTP, TLS or classical
  key package. A two-node join through the engine is C5.

## C5 implementation notes

- **Unit and module tests.** Restart cost independent of height, a pruned
  node passing its checks, a snapshot of a live chain restoring on a second
  database with the same application hash, a corrupted chunk, a wrong hash
  and an interrupted restore are covered by the C1 to C3 tests.
- **Two-node join.** `state-sync-join` (native-supervisor, behind
  `qualification-fixtures`) runs the real guarded binaries as a
  four-validator loopback chain from `dytallix-comet-fixture` with the PQC
  loopback transport. It is the owner parent of every child, using the
  release-runtime owner protocol unchanged; it installs neither
  no_new_privs nor a seccomp filter, so it runs where both are already
  applied. Nodes 0 to 2 commit and write snapshots every 5 blocks, keeping
  10 (synthetic values); node 2 keeps every block record so that it can
  serve blocks after the snapshot. After block 22, node 2 stops, its light
  blocks from 10 to its head are exported, and it restarts. Node 3 starts
  empty with state sync trusting height 10 and must log a restored snapshot
  within the export, catch up, and reach a later height whose application
  hash, from its own application, equals the chain's.
- **Runs.** The E02 native job runs it under a transient systemd unit with
  no_new_privs and the production `SystemCallFilter`.
  `tools/state-sync-join/run-in-docker.sh` does the same in a container with
  the `oci_seccomp.py` profile. Three local runs restored the snapshot at
  20 and matched the application hash at heights 28 to 30.
- **Observed.** Snapshots of one height differ between pruned nodes and the
  archive node, since block records are local; pruned nodes agree. The
  engine fetches chunks only from peers offering the exact snapshot.

The AppArmor roles (E02) must allow the bridge to read the snapshot
directory and the application to write it; that change returns to E02 and
its native checks.

## Decisions (P01, approved 27 September 2026)

1. Startup check: the current state completely plus the retained window
   (rule 1), not a replay from genesis.
2. History: a window derived from existing limits, with an optional archive
   node (rule 2).
3. Headers for a joining node: supplied by the operator locally (rule 5), not
   fetched over a new P2P channel.
