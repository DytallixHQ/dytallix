# Evidence and issuance liveness (E04)

Status: approved (P01, 27 September 2026) and implemented. Closes gaps 1 and
2 of the [E04.1 triage](../mainnet/e04-requirement-triage.md). Paths are
relative to `dytallix-fast-launch/node/src/`.

## Problems

| ID | Problem |
| --- | --- |
| L1 | Evidence stopped the chain. Without a penalty profile, required on mainnet today because that profile refuses vesting locks and production activation, any evidence made prepare and finalize fail. Light-client-attack evidence failed in every profile: the bridge refused it, and CometBFT panics when PrepareProposal fails. CometBFT verifies evidence before a block carries it, and the application cannot refuse it. |
| L2 | The issuance journal stopped the chain after `max_recorded_epochs` epochs, and every block reread every epoch record and replayed the controller from genesis (in the planner and again in the supply check). |
| L3 | A validator withdrawal stopped the chain (found 28 September 2026). Without the penalty profile, ordinary v2 admitted a `ValidatorWithdraw` from an unbond's owner, then failed it as an internal fault, so PrepareProposal and FinalizeBlock failed for any block carrying it. Any delegator who had begun unbonding could stop the chain with one transaction. |

## Decisions (P01, 27 September 2026)

1. Until penalty rules (D09) are decided, evidence is recorded only: an
   incident record, no effect on stake or the validator set.
2. The issuance journal keeps a window of the last `max_recorded_epochs`
   epochs plus a controller checkpoint, instead of stopping.

Decision 1 is superseded for duplicate votes by
[penalties v1](penalties-v1.md) (P01, 30 September 2026): the launch
configuration carries the penalty profile, so a first duplicate vote is
penalized with removal and withdrawals work. Light-client attacks stay
record-only.

## Rules

**Evidence.**

- The bridge forwards both CometBFT kinds, `duplicate_vote` and
  `light_client_attack`; an unknown kind is still refused.
- A duplicate vote under the penalty profile follows the existing penalty
  path, with its historical checks. Every other fact is checked for shape
  only and recorded under `evidence:v1:{height}:{index}` (committed state), in
  block order. Shape-only checks mean no historical mismatch can stop a block
  the engine verified.
- The per-block and complete checks require each recorded fact's record, and
  the complete check refuses records without a committed fact.

**Validator withdrawal.** Without the penalty profile principal withdrawal
stays disabled. A withdrawal is a paid rule failure,
`VALIDATOR_WITHDRAWAL_DISABLED` (result code 3): admitted, carried and
charged, with no custody change. Client receipt checks accept the code.

**Issuance.**

- `adaptive:v1:base` holds the controller state before the oldest retained
  event. When an epoch transition would keep more than `max_recorded_epochs`
  events, the oldest is replayed into the base and removed with its
  observation record; `TimingState.pruned_issued` keeps the issuance its
  command set.
- Blocks read only the timing state, its bindings, the head and the latest
  event. Supply reads only the timing state and its bindings. The complete
  check replays the window from the base to the head and reconciles the
  cumulative issuance.
- `max_recorded_epochs` becomes the audit window. Its genesis bound (at most
  1,000,000) is unchanged; it no longer stops the chain.

## Tests

- Validator withdrawal: without the penalty profile a withdrawal is admitted,
  proposed and accepted, and commits as a paid `VALIDATOR_WITHDRAWAL_DISABLED`
  failure with the principal still in unbond custody; before the fix
  PrepareProposal failed.
- Evidence: both kinds recorded without a penalty profile, with no set
  change, surviving a restart; a changed or extra record refused by the
  complete check; an unknown kind refused; under the penalty profile a
  light-client attack recorded and not penalized; the bridge forwards a
  light-client attack as its own kind.
- Issuance: a windowed journal matches the full journal's commands and
  controller head through twelve epochs with a window of three; a missing or
  changed base and a gap are refused. A consensus chain runs to epoch 19 with
  a window of 8, keeps 8 events and 8 observations, and passes the complete
  check after a restart. Removing the pruning or the pruned total fails the
  test.
