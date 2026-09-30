# Validator penalties and withdrawals v1 (E04)

Status: approved (P01, 30 September 2026: decisions below); PN-a to PN-d
implemented. Engineering task E04. Decision IDs refer to
`mainnet/launch/MAINNET_DECISION_REGISTER.json`. Paths are relative to
`dytallix-fast-launch/node/src/`.

This step puts the penalty profile on the launch configuration, so that
double-signing is penalized and unbonded stake can be withdrawn. It sets no
production numbers: the penalty rate, evidence age limits and margins are E05
inputs.

## Decisions (P01, approved 30 September 2026)

1. Faults (D09-Q04): double-signing only, with removal. A validator's first
   duplicate vote deducts a fixed share from all stake bonded to it at that
   height, self-bond and delegators alike. The validator is removed for good
   and its consensus key is never reused; later evidence against it adds
   nothing. Light-client attacks are recorded only. There is no downtime
   penalty.
2. Withdrawals (D09-Q05): an unbond can be withdrawn once both evidence age
   limits plus their margins have passed and no penalty on it is unsettled,
   from genesis.
3. Vesting-locked stake: bonded locked DGT carries the same risk as any
   stake. A penalty comes off the amount still locked, and the vesting dates
   stay the same.
4. Destination: penalized DGT moves to a penalty escrow that nothing can
   spend. The DGT total stays fixed (D05-Q02: no DGT burn).

## State before this step

The penalty profile (`runtime/penalty_custody.rs`) already implements rule 1
and the withdrawal rule, but the launch configuration could not use it:

| ID | Problem |
| --- | --- |
| PN1 | The profile refused every genesis with a vesting lock, in five places: `PenaltyState::initialize`, the block lifecycle, the stored-state check, the genesis check in `consensus_settlement.rs` and the supply check. The launch genesis has vesting buckets. |
| PN2 | Without the profile every withdrawal fails as `VALIDATOR_WITHDRAWAL_DISABLED` (liveness v1). Nothing required the profile, so a launch configuration without it would lock all stake. |
| PN3 | Removing the refusals alone would halt the chain. The lock check (`RewardState::liquid_spendable`, run per transfer and by the supply check) counts gross bonded and unbonding custody. When a penalized owner withdraws, the owner's holdings fall by the deduction; if they fall below the locked amount the supply check fails the block. |
| PN4 | Between a withdrawal and the next block start, gross unbonding custody still counts the released principal, which is already liquid. A locked owner could withdraw and then transfer locked DGT in the same block. |
| PN5 | Faulted-validator markers are kept permanently (retention decision 4) but capped at 64. Validator IDs come from the governance registry, so a 65th penalized validator over the chain's life would fail the block. |
| PN6 | A node could not restart once a penalized owner's tranche had been released (found writing the PN-a test). The incident then leaves state (retention), but the complete check required a receipt for every committed evidence fact it replayed. |

Already in place: only a duplicate vote is penalized, once per validator;
the validator's full exit is scheduled at H+2 and the deduction settles
then; admission refuses new exposure to a faulted validator as a paid
failure (`ValidatorExposureBarred`); used consensus keys are kept forever;
deductions go to `penalty_reserve`, counted in DGT custody by the supply
check, with no operation that spends it.

## Rules

1. **Lock relief.** The penalty state keeps `lock_relief`: for each owner
   with a vesting lock, the cumulative deductions of that owner's stake. A
   deduction adds to it in the block where it settles. It survives the
   pruning of released tranches. Owners without a lock have no entry, so the
   map is bounded by the genesis locks.
2. **Lock check.** For an owner with a lock, the locked amount at a time is
   the vesting schedule's locked amount minus the owner's lock relief, and
   never below zero. Custody is gross bonded and unbonding custody minus the
   released and deducted amounts of the owner's tranches that custody still
   holds. With both terms, a penalty lowers the owner's holdings and the lock
   by the same amount (PN3), and released principal is not counted twice
   (PN4).
3. **Checks.** The complete and per-block checks require the lock relief
   total to be at most the deducted total, each owner's relief to be at
   least the deductions of the owner's live tranches, and every relief entry
   to name an owner with a lock.
4. **Launch configuration.** The production application configuration
   carries the lifecycle and penalty profiles, with the recovery and
   ordinary profiles. The mainnet-preparation binding review reports
   `lifecycle_and_penalty_profiles` as missing otherwise, and E01 records the
   requirement. Local and test profiles may omit them.
5. **Faulted-validator markers** are bounded by the penalty state's item
   limit (10,000) instead of 64.
6. **Escrow.** `penalty_reserve` has no spending operation; the DGT supply
   view reports it as its own field, inside the fixed total.
7. **Pruned incidents.** The complete check accepts a committed evidence
   fact without a receipt only when the fact is past the horizon at the
   head's parent, the condition under which its incident was pruned. Tranche
   accounting still checks the deductions it made.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| PN-a (done) | Rules 1 to 3; the five refusals removed | Locked genesis runs the penalty profile |
| PN-b (done) | Rule 5 | No lifetime cap on penalties |
| PN-c (done) | Rule 4; runbooks | Launch configuration requires withdrawals and penalties |
| PN-d (done) | Rule 7 | A penalized chain restarts after its incidents are pruned |

## Tests

- A vesting-locked delegator bonded to a validator that double-signs: the
  deduction settles at H+2, the lock falls by the deduction, every block
  passes the supply check, the owner withdraws the net principal after
  maturity, and a restart after the incident is pruned passes the complete
  check (PN6). Before PN-a the genesis
  was refused; with the refusals removed and no relief, the withdrawal block
  failed the supply check.
- In the block of that withdrawal the owner cannot transfer more than the
  unlocked amount (PN4).
- Changing or removing a relief entry is refused by the complete check.
- A state holding 65 faulted-validator markers validates and round-trips
  (PN5).
- The binding review reports `lifecycle_and_penalty_profiles` as missing
  for a configuration without both.
