# Full consensus halt

Runbook v1 ([index](README.md)). This procedure was the full-halt draft. It
authorizes no live halt, restart, key operation, rollback or state
migration. The halt and resume controllers, thresholds, communication
authentication and checkpoint acceptance are unset inputs (D14-Q03,
D12-Q02).

Use this procedure when continued consensus, deterministic execution or
cryptographic verification is unsafe. Use it also when the chain has
stopped by itself because every validator fails the same block.

A transaction freeze ([emergency transaction freeze](../mainnet/emergency-transaction-freeze.md))
stops user transactions while consensus continues, and cannot repair those
conditions. A freeze needs a new block to finalize, so do not depend on one
after consensus has stopped.

## Signals

- `dytallix_engine_consensus_latest_block_height` stops rising on every
  validator. `dytallix_engine_consensus_rounds` keeps rising at the stuck
  height.
- Supervisors on every validator stop at the same height. The same
  `application_failure_class` appears everywhere, most often `execution`,
  `supply` or `history`.
- An operator decides that signing must stop, for example after a
  confirmed key compromise ([key-compromise.md](key-compromise.md)) or a
  conflicting commit ([fork.md](fork.md)).

## Sequence

1. **Record.** Record the incident, the affected candidate hashes and the
   evidence. Use the approved independent communication channel. Identify
   which safety assumption is uncertain.
2. **Authorize and coordinate.** Obtain the required halt authorization and
   coordinate the affected validator operators. If an operator must stop an
   unsafe signing process at once, record that action separately from the
   network-wide authorization.
3. **Fence each stopped signer.** Stop the supervisor and preserve:
   - `data/priv_validator_state.json` (the last signed height, round and
     step);
   - the application database and the engine data;
   - the supervisor report.

   Do not reset signing state or start a replacement signer at the same
   time.
4. **Find the last trustworthy checkpoint.** Base it on independently
   checked commit evidence: exported light blocks, and each node's
   `dytallix-state-check` result with its height and `app_hash`. Record any
   disagreement. Never choose a checkpoint because one node reports it.
5. **Diagnose and correct the cause.** Bind the correction to reviewed
   source, executable hashes, cryptographic profile, state compatibility
   and test evidence. Keep the previous records for review.
6. **Obtain resume authorization.** It is separate from the halt and is
   bound to the accepted checkpoint, the release and the recovery evidence.
   A halt signature does not authorize resumption, replacement keys,
   spending or state rewriting.
7. **Verify before signing.** Every intended operator has the same
   accepted checkpoint and release. Signer ownership is exclusive and
   signing state is compatible.
8. **Restart.**
   - If the cause was outside the committed release (a host, network or
     operator fault), restart on the same release, in the coordinated
     order.
   - If the correction is new code, follow [restart.md](restart.md). The
     application refuses any release but the committed one, and a handover
     needs a block. A root-signed restart authorization is the only way to
     run a fixed release at the halted height.

   Never write a freeze, a handover or any other state into the database by
   hand.
9. **Verify the resumed chain.** Check that:
   - the finalized state is consistent across nodes;
   - `dytallix_engine_consensus_latest_block_height` rises on every
     validator;
   - peer admission, signer exclusivity and the required monitoring hold.

   Resolve any transaction freeze only through its own authorized resume
   control.

## Do not

- Create a replacement genesis, change the chain identity or revert
  finalized history as a recovery step. These need explicit decisions
  outside this procedure.
- Treat a timeout as authorization to restart.
- Restart a node repeatedly on a deterministic failure. It replays the same
  block and fails the same way.

## Remaining inputs

- Halt and resume controllers, thresholds and communication
  authentication.
- Checkpoint verification rules and signer fencing evidence.
- The handover custodians who sign a restart, and the failure criteria.

Exercise the procedure on disposable local or assigned staging systems
before acceptance. Production private keys do not belong in the evidence
package.
