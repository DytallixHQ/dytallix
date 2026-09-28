# Restart on a new release (E04 gap 18)

Engineering task E04, gap 18 of the [E04.1 triage](../mainnet/e04-requirement-triage.md)
(OBS-003, KEY-004). It was found while writing the incident runbooks (gap 15).

## Problem

Two kinds of halt stop the chain at height H on every validator:

1. **Decided, not executable.** Block H is decided, but every validator
   fails to execute it. The engine replays it on every restart and fails
   the same way.
2. **Undecidable.** No block H can be proposed or accepted, because every
   validator's PrepareProposal or ProcessProposal fails.

Either way the fix is new code, which means a new release. The application
refuses any executable but the committed active release
(`ensure_active_runtime`), and only an on-chain handover changes that
release. A handover needs a block, and the halt prevents one. No existing
path resumes the chain.

## Decisions (P01, 28 September 2026)

1. **A restart authorization.**
   - It is a root-signed artifact that names:
     - the halted height H, and block H's hash when it was decided;
     - the last committed checkpoint (H−1 and its application hash);
     - the committed release and the new release.
   - Operators supply it as a pinned file at startup. The application
     verifies it with the root helper, runs the new release for block H and
     records the switch and a receipt in block H's state.
   - History is never rewritten. The chain resumes only if more than two
     thirds of the voting power runs the new release with the
     authorization.
2. **Handover authority.** The release handover's key set and threshold
   sign it, through the root helper under the `upgrade` action with its own
   artifact domain. It consumes one handover sequence, so it is ordered
   with handovers and cannot be replayed.
3. **Release only.** It switches the active release with the state schema
   preserved. It carries no migration and no state edits: block H's effects
   are whatever the new release computes. A migration uses the upgrade path
   after the chain resumes.
4. **Resume open.** The chain resumes as it was, and a freeze in force stays
   in force. A freeze or resume remains a separate control.

## Contract

**The artifact.** The bytes are `DYTALLIX/RELEASE-RESTART/v1\0` followed by
the canonical JSON payload. The payload binds:

| Field | Value |
| --- | --- |
| `schema` | 1 |
| `chain_id`, `genesis_sha256`, `policy_sha256`, `authority_epoch` | The handover policy's |
| `sequence` | The handover state's next sequence |
| `source_release_sha512` | The committed active release |
| `target_release_sha512` | The new release; it differs from the source |
| `schema` of the state | The active schema, unchanged |
| `parent_height`, `parent_app_hash` | The committed head: H−1 and its application hash |
| `halted_height` | H = `parent_height` + 1 |
| `halted_block_hash` | Block H's engine hash when H was decided; absent when it never was |
| `emergency_receipt_sha256` | The latest emergency receipt, or absent: the current emergency history |
| `pending_admission_receipt_sha256` | The pending handover admission it supersedes, or absent |
| `evidence_sha256` | The incident evidence digest |

**Signatures.** At least the handover threshold of distinct, sorted
handover keys. Each is verified by the root helper under the `upgrade`
action, with the sequence and the height window H..H.

**Validity.** It is valid only when all of these hold:
- the committed head is exactly H−1 with that application hash;
- the committed active release is the source and the schema is unchanged;
- the next handover sequence is its sequence;
- the emergency history and any pending admission are the ones it names.

A freeze does not block it.

**Delivery.** The operator pins the file in the service configuration. The
application takes `--restart-authorization FILE`. It verifies the file at
startup and then accepts the target release as the running candidate for
block H. The supervisor's preflight selects the target release the same way.

**Execution of block H.**
- The switch applies before the block's transactions. The new release
  executes block H.
- The block's write batch carries:
  - the new handover state: the target active release, the sequence
    advanced, the pending admission cleared, and this receipt as the last
    and the activation receipt;
  - the restart receipt.
- When `halted_block_hash` is set, only that block is accepted at H.
- Block H may carry no handover control.
- Emergency controls in block H bind the release active at H−1, as they do
  in a handover's activation block.

**After block H.**
- The committed active release is the target, and the source refuses to
  run (exit class `release`).
- A supplied authorization whose receipt is already committed is ignored,
  so the operator can remove the pin afterwards. Any other one is refused.

**History.**
- The restart receipt is stored under the handover namespace, keyed by
  sequence. It holds the authorization and its prior receipt.
- Startup replays it with the root helper. It must bind block H's record:
  the record's prior application hash, and its engine hash when set.
- The derivation of the active release at each height, which emergency
  controls use, includes it.
- Block H is pinned against pruning, as blocks with recorded controls are.

**Limits.**
- A node that block-syncs across H runs the source until it stops at H,
  then the target with the authorization. A node that state-syncs past H
  needs neither.
- The target release must execute every earlier block as the source did.
  The authorization cannot prove that, so release review must.

## Steps

| Step | Content |
| --- | --- |
| S-a | This contract; the restart type, validation, signature verification, state transition and replay (`release_handover::restart`) with unit tests |
| S-b | The application: the flag, startup verification, block H execution, history replay and pruning pins, the status view; a process test of the switch between two builds |
| S-c | The supervisor's pinned input and preflight; the runbooks (halt, supply mismatch, upgrade failure) replace their gap 18 stop |
