> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Token supply model

**DRAFT NOT FROZEN. This document does not approve mainnet activation.**

APPROVED means the user selected the rule within the recorded scope. OBSERVED means the inspected source or evidence contains the rule. PROPOSED means an engineering recommendation awaits approval. MISSING means the required decision or evidence has not been supplied. Approval of a policy does not prove implementation.

## Units and custody

**APPROVED — EC-01.** Fixed total: 1,000,000,000 DGT. Allocation shares: 30/20/15/15/20. DRT reward shares: 40/30/30.

**APPROVED — EC-02 decimal scale.** Both tokens use six decimals. The fixed DGT total is `10^15 udgt`. The [Batch 6 approval](batch-6/APPROVAL.json) approves the scale separately from the earlier allocation approval. Full issuance at genesis remains unapproved.

**PROPOSED — remaining EC-02 encoding rules.** Serialize nonnegative integer amounts as canonical decimal strings. Use checked arithmetic for multiplication, addition, and subtraction. Reject overflow. Do not use floating-point balances or saturation that silently loses value.

**PROPOSED — EC-04.** Partition custody as follows. Each base unit belongs to exactly one row for its denomination. Ownership and vesting restrictions are metadata, not additional balances.

| Token | Symbol | Exclusive custody category |
|---|---|---|
| DGT | Lg | Liquid accounts |
| DGT | Kg | Locked, not bonded or unbonding |
| DGT | Bg | Bonded stake, including any permitted locked stake |
| DGT | Ug | Unbonding stake, including any permitted locked stake |
| DGT | Gg | Governance deposit escrow |
| DGT | Pg | Penalty escrow pending approved disposition |
| DRT | Lr | Liquid accounts, including liquid treasury accounts |
| DRT | Kr | Locked accounts outside reward or fee custody |
| DRT | Fr | Fees moved into committed settlement escrow |
| DRT | Vr, Sr, Tr | Validator, staker, and treasury reward pools |
| DRT | Xir | Separate issuance-split reserve |
| DRT | Xsr | Separate staking-recipient rounding reserve |
| DRT | Zr | Inactive-period reward reserves |

**PROPOSED — EC-04.** A mempool reservation is a restriction on a liquid balance. It is not a second custody balance. A committed transfer to fee escrow moves the balance from `Lr` to `Fr`. Reward claim records are liabilities backed by `Vr`, `Sr`, or `Tr`; do not add them to supply again. Treasury ownership can describe `Lr` or `Tr`, but never adds another balance. The controller's uncommitted command is neither issuance nor custody.

**PROPOSED — EC-04.** Reject any monetary transition that cannot map to this versioned partition. New token custody requires an approved schema migration and new invariants. Do not hide unexplained balances in an unrestricted other-custody category.

## Conservation equations

**PROPOSED — EC-03/04.** At every committed height `h`, with all values in base units:

```text
Cg(h) = Lg + Kg + Bg + Ug + Gg + Pg
Cg(h) = Ig + Mg(h) - Dg(h)

Cr(h) = Lr + Kr + Fr + Vr + Sr + Tr + Xir + Xsr + Zr
Cr(h) = Ir + Mr(h) - Dr(h)
```

`I` means genesis-issued supply. `M` means cumulative recognized post-genesis minting. `D` means cumulative recognized destruction. A transfer, lock, delegation, unbonding, deposit, or reward claim leaves supply unchanged. Burn counters are historical counters and are not live custody.

**PROPOSED — EC-03.** Use `Ig = 10^15 udgt`, `Mg = 0`, and `Dg = 0` after approval of the remaining EC-03 issuance and burn rules. The generic DGT equation exposes prohibited changes. It does not authorize DGT minting or burning. **MISSING — EC-11.** `Ir` and any DRT supply limit need approval.

**PROPOSED — EC-04.** Genesis category totals and custody totals must independently equal `Ig`. The five category totals partition beneficiaries; the custody categories partition state. Do not add those two views together. Derive each view from the same allocation records.

**APPROVED — EC-08 recipient allocation arithmetic.** For each supplied interval budget and eligible-stake snapshot with total weight greater than zero, use `floor(budget * weight / total_weight)` with checked wide arithmetic. A sole eligible recipient receives the entire budget. Record the unallocated integer remainder in a separate reward-pool reserve. Do not assign an automatic recipient or sweep authority. The [later integration approval](batch-6/integration-followup/APPROVAL.json) adds finalized-block timing, parent-state eligibility, owner entitlements, inactive-period retention and rejection of unconverted legacy state. It does not authorize a legacy conversion plan or production activation.

**APPROVED — EC-07 epoch budget distribution.** The [issuance timing approval](batch-6/issuance-timing/APPROVAL.json) adopts an explicit positive finalized-block count `N` and explicit epoch-zero command `E0`. Split each command `E` into `floor(4E/10)`, `floor(3E/10)`, `floor(3E/10)` and the remaining `issuance_reserve` budget. Schedule each of the four budgets as `floor(B/N)` per block plus one base DRT unit in the first `B mod N` blocks. The four complete block schedules sum to `E`. Later commands use the completed previous epoch's observation bound to the expected parent. Numeric production inputs remain unselected.

**PROPOSED — remaining EC-08/12 transition model.** The first two equations below express approved budget conservation. The remaining custody and fee model still requires its stated approvals:

```text
epoch E = sum(block_validator_credit + block_staker_credit + block_treasury_credit + block_issuance_reserve_credit)
pool budget B = sum(new integer entitlements) + pool_rounding_credit
fee reservation = refund + fee_recipient_credits + burn + retained_fee_credit
claim pool debit = liquid beneficiary credit
bond liquid_or_locked_debit = bonded_credit
unbond bonded_debit = unbonding_credit
exit unbonding_debit = liquid_credit + still_locked_credit
governance deposit debit = governance_escrow_credit
penalty stake debit = penalty_escrow_credit
```

Keep `issuance_reserve`, staking-recipient rounding and inactive staking reserves separate. None has an automatic recipient or sweep authority. In this disjoint model, `Sr` excludes `Xsr` and `Zr`. If a stored staking pool contains all three, reconcile its balance as unpaid entitlement backing plus the two staking reserves. Do not add the reserves to that stored balance again.

The pool budget is the new interval allocation, not all funds in a pool that also backs old entitlements. The pool-budget equation assigns liabilities within existing pool custody. Recording entitlements must not credit the pool a second time. A burn debits custody and increments the destruction counter by the same amount. A pool-to-pool move does not mint.

## Atomic commit and authority

**PROPOSED — EC-04.** Each transition record includes chain identifier, genesis digest, protocol version, height, transaction or epoch identifier, denomination, source and destination custody identifiers, amount, supply delta, authorization reference, and receipt. Record residuals with their source interval and policy version. Authenticate claim ownership through the approved account policy.

**APPROVED — EC-04/06 atomic issuance boundary.** Commit controller journal, accepted epoch and block identity, command consumption, four bucket credits, reward state, supply counters and block settlement in one storage batch. A duplicate command cannot mint again. A rejected or failed commit leaves all state unchanged. Replay and restart must preserve custody, counters and receipts. Production qualification remains open.

**APPROVED — development observation failure.** Reject the epoch-boundary block when the required previous-epoch observation is missing, invalid or bound to the wrong parent. Use no substitute input or catch-up issuance. Production oracle authentication, aggregation and pause-and-resume rules remain unapproved.

**PROPOSED — EC-10/12.** Governance can change only approved parameters within reviewed domains. Use a timelock and an effective height. A model or timebase change needs an explicit state migration. Do not let an ordinary configuration update rewrite old reward entitlements, create custody, or change the fixed DGT total. Full initial governance remains required.

## Integer examples

**PROPOSED — arithmetic illustrations only.** These inputs are not genesis parameters or emission settings.

| Case | Expected result |
|---|---|
| Issuance E = 1 udrt | V=0, S=0, T=0, issuance_reserve=1 |
| Issuance E = 7 udrt | V=2, S=2, T=2, issuance_reserve=1 |
| Issuance E = 10 udrt | V=4, S=3, T=3, issuance_reserve=0 |
| Pool B = 250000 udrt, one eligible weight | Entitlement 250000; payout residual 0 |
| Pool B = 7 udrt, eligible weights 1 and 2 | Entitlements 2 and 4; payout residual 1 |
| Pool B = 7 udrt, no eligible weight | Inactive-period reserve 7; entitlement 0 |
| Fee reservation 100 udrt; compute 40, bandwidth 15, tip 5 | Refund 40; recipient 45; burn 15; supply falls 15 |

**APPROVED — EC-08 reward-pool remainder only.** The unallocated integer remainder stays in a separately recorded reward-pool reserve. It has no automatic recipient or sweep authority.

**APPROVED — inactive staking reserve.** The integration follow-up retains budgets for intervals with no eligible stake in a separate reserve. It has no automatic recipient or spending authority.

**APPROVED — EC-07 issuance-split reserve.** Retain the epoch split remainder in `issuance_reserve`. Schedule it across blocks by the same even-distribution rule as the other three buckets. It has no automatic recipient or sweep authority. The complete custody model and storage-growth qualification remain open.

**OBSERVED — integration status.** The development implementation combines the explicit epoch budget schedules with reward liability changes, pool debits, account credits and the controller journal. It does not establish production consensus finality or select numeric production parameters. Final issuance timing evidence remains separate from earlier follow-up results. The unchanged legacy reward failure remains open.

## Repair and acceptance order

**OBSERVED.** [Batch 1 failure evidence](batch-1/TEST_FAILURES.json) has nine economic cases: three genesis, five governance, and one staking reward. Their count is a subset of the 14 workspace failures.

1. **MISSING:** Assign the economics owner and independent reviewer. Approve the remaining EC-02/03/05/07/08/09/10/11/12 terms with exact values where required. Batch 6 and its follow-ups approve the recorded unit, reward, eligibility, reserve, issuance timing and atomic settlement rules. Production values and other economic terms remain open.
2. **PROPOSED:** Generate one native allocation and custody fixture from those approved rules. Keep historical core fixtures visibly separate.
3. **PROPOSED:** Implement custody transitions and checked unit conversion. Reconcile governance snapshots and reward allocation against the reference rules.
4. **PROPOSED:** Re-run all nine baseline cases. Test exact conservation, overflow rejection, duplicate settlement, empty participation, stake changes, vesting boundaries, failed commits, and restart.
5. **PROPOSED:** Compare independent reconstructions of all custody sums against supply counters. Bind the results to source, genesis, and binary digests.
6. **MISSING:** Complete economic model qualification, production integration, independent assurance, and the seven full launch rehearsals. Local arithmetic or repaired fixtures do not satisfy these gates.

**OBSERVED.** Batch 2 arithmetic checks cover the approved category totals and the illustrative settlement equations only. [ECONOMIC_ARITHMETIC.json](batch-2/evidence/ECONOMIC_ARITHMETIC.json) records results. [ECONOMIC_DECISIONS.json](batch-2/ECONOMIC_DECISIONS.json) records decision dependencies and source hashes. No native runtime repair or launch approval follows from this model.
