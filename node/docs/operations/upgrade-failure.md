# Upgrade or handover failure

Runbook v1 ([index](README.md)). The roles and channel are unset (D14-Q03,
D12-Q02). The authority sets, thresholds and limits are E05 inputs.
Production upgrade activation and production handover are not yet
qualified.

The chain changes code or state format only through two root-signed
control types:

- **Upgrade** ([upgrade execution](../mainnet/upgrade-execution.md)):
  admit a plan, activate it at a target height, or cancel it. The only
  migration is `emergency-receipt-digest-index-v1`.
- **Release handover:** admit a target release, activate it, or cancel it.
  After activation, the committed active release is the target, and every
  node must run the target's executable.

Each control writes in the block's single synchronous batch. A failure
before the write changes nothing, and a lost acknowledgement replays the
committed result exactly.

## Signals

| Signal | Case |
| --- | --- |
| Before activation: a control is refused, or the plan is wrong | 1. Before activation |
| After a handover activates, validators still on the old release exit `release` (14). The old executable refuses every later block and control ("Committed release requires a different runtime executable") | 2. The node was not switched |
| An executable other than the committed release exits `release` (14) at startup ("Candidate manifest digest mismatch", "Candidate executable ...") | 3. Wrong executable |
| A node exits `replay` (13) at startup: the upgrade, handover or emergency history does not replay | 4. History does not replay |
| A node exits `configuration` (10) after a configuration or genesis file changed | 5. Changed inputs |
| The new release fails blocks or cannot start on every validator | 6. The new release is broken |

## 1. Before activation

- **Abort.** Abort with a signed `Cancel` of the pending admission, which
  is an upgrade or handover control. Nothing else changes.
- **Holds.** A freeze sets the upgrade hold, and a resume leaves it set. A
  fresh activation authorizes only its own named plan under the current
  emergency history. A later freeze invalidates it.
- **Expiry.** An expired activation needs a new authorization; nothing
  runs automatically.

## 2. The node was not switched

1. Confirm the committed active release. The application's status view
   (`release_handover.active_release_sha512`) on any node running the target
   shows it.
2. Start the node on the target release's executable. The supervisor
   selects the candidate from the verified committed authority, and the
   application verifies the executable against the release manifest when
   it opens.
3. Do not restart the old executable. It will keep exiting `release`.

## 3. Wrong executable

The installed executable, its manifest or its catalog entry is not the
committed release.
1. Compare the executable's hashes with the release record.
2. Reinstall the committed release's artifacts from the approved
   distribution. Never edit the manifest to match a binary.

## 4. History does not replay

A node's stored controls no longer replay under the real helper. This covers
missing, extra or altered upgrade and handover records, or a wrong index.
1. Run `dytallix-state-check`. It checks everything except the replays,
   which need the owned root helper; a `pass` narrows the fault to them.
2. If only this node fails, recover it as in [fork.md](fork.md), case A.
   If every node fails, follow [halt.md](halt.md).

## 5. Changed inputs

The application configuration and genesis are bound to storage from the
first block, and the supervisor pins them by hash. Restore the exact files
of the release record. A changed profile or limit is a new chain, not an
upgrade.

## 6. The new release is broken

After activation, the only way back is another handover, to the previous
release: a schema-preserving return. That handover needs a block.
- If the target release still produces blocks, admit and activate the
  return handover.
- If it cannot produce blocks, the chain is halted and neither release can
  run. Follow [halt.md](halt.md). Resuming on a working release needs the
  restart mechanism of gap 18.

## Do not

- Run an older executable against state its release did not write, unless
  a signed return handover has made it the committed release.
- Write upgrade or handover state into the database by hand.
- Replace signing state to get a node past a refused block.
