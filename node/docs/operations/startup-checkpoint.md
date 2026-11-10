# Startup checkpoint and the no-peer rule (T06)

Incident 2026-11-10: the staging sentry (archive, height 173,074) failed
`BOUNDED_STARTUP_FAILURE`. Its application answers the supervisor's `info`
only after its open, and the open checked every block from 1, twice. That
took longer than `process.startup_millis` (at most 60 s). Investigation:
draft PR #135.

## What changed

- **Checkpoint.** The application keeps a node-local record,
  `local:v1:startup-checkpoint` (version 1): height `c`, the engine and
  application hashes of record `c`, the height of the last complete check,
  and the heights at or below `c` whose records carry a control. It is not
  committed state, is not in snapshots, and a restored snapshot may not carry
  one. It is written after a passed startup check, after the complete check
  of `dytallix-state-check --write-startup-checkpoint`, and after each
  commit's per-block check, extended only from the parent. A node that has
  removed records (a window) ignores it: its window already bounds startup.
- **One check per start.** With `--candidate-config` the read-only preflight
  runs the startup check. The writable open skips its own check only if the
  database is the one the preflight checked: same RocksDB `IDENTITY`,
  sequence number, head and checkpoint. Otherwise it runs it again. With a
  V2 candidate the one check uses the verifier the node starts with, not the
  candidate's live verifier, which the second pass used to.
- **No-peer rule** (supervisor readiness). A production sentry or endpoint
  whose engine reports no peer (`net_info` on the operator socket) on every
  poll for one whole startup bound, at an unchanged height, is ready although
  it is still catching up. A peer, or a height change, restarts the window.
  The receipt shows `"peerless": true`. Validators never use the rule, and
  readiness never moves the engine out of block sync, so the rule cannot make
  any node sign. The signing guard (`host_install.py guard H`, validator
  recovery) is what stops a replacement signing at or below H.

## What startup checks

With a checkpoint at `c`, startup still runs:

- the configuration, genesis and root receipt bindings;
- every state-wide check: all accounts and supply totals, the issuance
  journal, governance, the recovery book, the state tree rebuilt from every
  entry, and the scan for unknown keys;
- record `c` against the checkpoint, and record 1 against the stored genesis
  commitment;
- every record after `c`, in full;
- the records the checkpoint lists as carrying a control, the anchors their
  receipts name, and the emergency, root authority, upgrade and handover
  replays over them, with the verifier.

## What startup no longer checks

For the records below `c` other than record 1 and the listed control
records, startup does not re-read:

- each record's own commitments (input and result digests, application hash
  formula), input limits, and parent and time links;
- receipts of those blocks against their inputs, including the signature
  checks of their ordinary and recovery transactions;
- evidence timing against the validator sets of that time, penalty incidents
  admitted at those heights (checked against state only), and the count of
  evidence records for those heights;
- the epoch observations those blocks derive;
- that each record key at or below `c` belongs there.

These are the checks a window node already skips for the records it has
removed (state sync v1). A change to those records does not change state;
the state must still rebuild to the head's committed root, whose application
hash the network's signed headers bind. No secret binds the checkpoint: a
writer with access to the stopped database can move it forward, and then
startup skips more. Treat the database as trusted as before.

## Where those checks now run

`dytallix-state-check` runs the complete check over every record, read only.
`--write-startup-checkpoint` runs the same complete check, then writes the
checkpoint at the head. Both need the node stopped; the writable one is
refused while the node holds its database. Neither runs the root helper:
the verifier replays run at every start instead. A complete check also runs,
as before, after any write outside commit.

## Bringing up a node with a long history

A data directory from before the checkpoint has none. Its first start runs
the complete check, as before, and may exceed `startup_millis`: the
supervisor then stops it (`BOUNDED_STARTUP_FAILURE`). Run the complete check
once instead, outside the startup budget:

1. Install a release that has the checkpoint (its `dytallix-state-check`
   prints `--write-startup-checkpoint` in its usage).
2. Stop the node: `systemctl stop dytallix-node`.
3. As the application owner:

   ```text
   /opt/dytallix/RELEASE/bin/dytallix-state-check --config APPLICATION_CONFIG \
     --genesis NATIVE_GENESIS --db HOME/appdb --write-startup-checkpoint
   ```

   It prints `"result":"pass"` with `startup_checkpoint_height`, the head.
   A failure prints its class; do not start the node.
4. Start the node. Its startup checks only what came after the checkpoint.

For the staging sentry from 2026-11-10 (173,074 blocks) the measured
0.19 ms a block (debug build, empty blocks) gives at least 33 s; expect more
on a shared 4 GB host. The command has no time bound.
