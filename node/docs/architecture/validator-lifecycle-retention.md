# Validator lifecycle retention (state model step 4)

Status: approved (P01, 27 September 2026: all four decisions below as
proposed). Engineering task E04. Covers problems P11
and P13 of [state model v2](state-model-v2.md) and the related penalty and
reward records. Paths are relative to `dytallix-fast-launch/node/src/`.

This step changes how long records are kept, not who pays or when. Penalty
rates, delegator liability and evidence types (D06, D09) stay open and
unchanged.

## Problems

Each of these stops the chain once a lifetime count is reached. None is
pruned today.

| ID | Record | Growth | Limit |
| --- | --- | --- | --- |
| L1 | Validator-set history (`LifecycleState.history`) | One full copy of every validator identity (1,952-byte keys) and every owner position, per block with any stake change | 10,000 entries; 16 MiB lifecycle blob |
| L2 | Validator update history (`update_history`) | One entry per block with a power change | 10,000 entries |
| L3 | Unbond entries (`unbonding`) | One per unbond, kept after withdrawal; count must equal `next_unbond_id` | 10,000 unbonds ever |
| L4 | Penalty tranches, incidents, release receipts (`penalty_custody.rs`) | One tranche per bond and per partial unbond; tranche count must equal `next_tranche_id` | 10,000 each; 16 MiB penalty blob |
| L5 | Staker slots (`reserved_owners`) and gross per-owner unbonding (`RewardState.unbonding`) | An owner is reserved forever; gross unbonding never decreases | `max_positions` distinct stakers ever |

Also, per block, `LifecycleState::validate` walks every history entry and
every unbond, and `PenaltyState::validate` walks every tranche and incident.

## Facts that shape the design

- **Evidence age.** Evidence is rejected only when it is older than both
  `evidence_max_age_blocks` and `evidence_max_age_seconds`
  (`PenaltyState::assess`, `consensus_settlement.rs` evidence times).
- **Withdrawal maturity.** An unbond can be withdrawn only after both limits
  plus the processing margins have passed since its last exposure
  (`UnbondEntry::maturity_satisfied`). So the evidence window and the
  withdrawal window are the same horizon.
- **What evidence needs from history.** At the offence height it needs each
  validator's consensus address and voting power, and the total power. Owner
  exposure comes from penalty tranches (`exposed_at`), not from the history
  view.
- **Supply uses running totals.** DGT conservation checks gross unbonding =
  net + deducted + released, and the penalty reserve = total deducted
  (`supply.rs`).
- **Key reuse.** Today a consensus key used by any past validator can never
  be registered again (`check_new_key`).
- **History verification.** The complete check compares every committed
  block's validator updates with lifecycle history. The same check can fold
  the committed updates forward from the genesis validator set instead.

## Proposed rules

1. **Horizon (no new parameter).** A record is kept while evidence or a
   withdrawal could still need it: until both evidence limits plus the
   processing margins have passed since the last height it covers. This is
   the existing maturity rule, applied to history.
2. **History entries (L1, L2).** Keep, per height with a change, each
   validator's consensus address and power (about 60 bytes per validator,
   at most 64 validators). Full keys stay only in the effective and scheduled
   sets. Entries move out of the lifecycle blob into per-height storage
   entries, read by height, and are dropped once past the horizon. The
   genesis set is kept. Update history is kept for the horizon; the
   complete check folds older updates from committed blocks.
3. **After a withdrawal (L3, L4).** The unbond entry, its tranches and its
   release receipt are removed once released. Counters keep the released and
   deducted totals, so supply conservation and the penalty reserve are
   unchanged. Unbond and tranche IDs stay unique through their sequence
   counters rather than a count check.
4. **Incidents (L4).** A settled incident is removed once its evidence is
   past the horizon (it can no longer be resubmitted). First-fault markers
   (at most 64, one per validator) are kept permanently, so the first-fault
   rule is unchanged.
5. **Staker slots (L5).** An owner's slot is released when it has no bonded,
   pending, unbonding or unpaid amount and no lock. Gross unbonding in the
   reward state drops by the released amount.
6. **Used consensus keys.** Keep a permanent set of used consensus addresses
   (20 bytes each), so the no-reuse rule is unchanged after history is
   pruned.
7. **Per-block checks** cover the entries a block changes plus the horizon
   bookkeeping; the complete check still checks everything that is kept.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| V1 (done) | Counters for released and deducted totals; unbond and tranche sequences checked by counter, not count | None |
| V2 (done) | Remove released unbonds, their tranches and receipts at the next block start; release empty staker slots; reward gross unbonding net of releases | State layout; capacity |
| V3 (done) | Compact history: a base set plus one change per activation (identity changes and power changes, with the parent time); used-address set; historical set lookups by height; update history kept for the last two heights | State layout |
| V4 (done) | Horizon pruning of history and settled incidents; merging of held tranches no admissible evidence can tell apart; the complete check folds committed validator updates from the genesis set and checks old blocks' evidence against the folded sets | Capacity |

**Implementation notes (V3, V4).**

- History stays in the lifecycle state rather than per-height storage. An
  entry is a marker or a power change (tens of bytes), full keys appear only
  when an identity changes, and the horizon bounds the count, so the
  existing 16 MiB state bound suffices. Scheduling beyond
  `MAX_HISTORY_CHANGES` (100,000 retained activations) is a paid capacity
  failure, not a halt. Per-height storage remains an option if production
  horizons need it.
- Every activation records an entry, even one that changes no power, so the
  horizon advances with time rather than only with power changes.
- Held tranches of one owner on one validator that started at or before the
  oldest retained history height are merged into the earliest one. Offences
  before that height are past the horizon, so no admissible evidence can
  tell them apart, and unbond order is unchanged. Tranches that were
  deducted, released, unbonded or allocated are never merged.
- A settled incident whose deduction still sits in a held tranche is kept
  until that tranche is released. Only first faults allocate, so there are
  at most 64.
- Two lifetime caps found during implementation were also fixed: new
  unbonds were refused once `next_unbond_id` reached 10,000 (now the live
  count), and reaching the tranche cap inside an action was an internal
  error (now a paid capacity failure).
- History entries keep no owner positions. The lifecycle check that an
  unbond was covered by the owner's historical position is replaced by the
  penalty custody's tranche exposure check.

Each step keeps the node suite green, with test oracles comparing the new
records with a full-history reference.

## Tests

- 10,001 bonds, unbonds, withdrawals and set changes on one chain.
- Evidence at the oldest admissible height is still accepted after pruning;
  evidence just past the horizon is rejected, exactly as before.
- Supply conservation and the penalty reserve are unchanged through
  withdrawals and pruning.
- A pruned node reopens and passes the complete check.
- Determinism across independent databases.

## Decisions (P01, approved 27 September 2026)

1. Retention horizon = the existing evidence limits plus margins (rule 1),
   with no new parameter.
2. Remove withdrawn unbond records and settled, expired incidents, keeping
   running totals (rules 3 and 4).
3. Keep the permanent no-reuse rule for consensus keys (rule 6), rather than
   allowing reuse after the evidence window.
4. Release a staker slot when the owner holds nothing (rule 5), so
   `max_positions` limits concurrent stakers rather than lifetime stakers.
