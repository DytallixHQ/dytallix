> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# DGT tokenomics

**DRAFT NOT FROZEN. This document does not approve mainnet activation.**

APPROVED means the user selected the rule within the recorded scope. OBSERVED means the inspected source or evidence contains the rule. PROPOSED means an engineering recommendation awaits approval. MISSING means the required decision or evidence has not been supplied. Approval of a policy does not prove implementation.

## Approved allocation

**APPROVED — EC-01.** The fixed DGT total is 1,000,000,000 DGT. The user designated the [tokenomics page](https://dytallix.com/developers/tokenomics) as the allocation source. [The saved page record](evidence/TOKENOMICS_APPROVED_SOURCE.json) records the approval scope and retrieval time.

| Category | Approved share | Approved DGT amount | Approved udgt amount at six decimals |
|---|---:|---:|---:|
| Ecosystem growth | 30% | 300,000,000 | 300,000,000,000,000 |
| Team and advisors | 20% | 200,000,000 | 200,000,000,000,000 |
| Public sale | 15% | 150,000,000 | 150,000,000,000,000 |
| Private sale | 15% | 150,000,000 | 150,000,000,000,000 |
| Reserve | 20% | 200,000,000 | 200,000,000,000,000 |
| Total | 100% | 1,000,000,000 | 1,000,000,000,000,000 |

**APPROVED — EC-02 decimal scale.** Both DGT and DRT use six decimal places. The fixed DGT total is 1,000,000,000,000,000 base units (`udgt`). The [Batch 6 approval record](batch-6/APPROVAL.json) approves this scale separately from the earlier webpage allocation approval. The table combines these two approvals. It does not approve full issuance at genesis.

**PROPOSED — remaining EC-02 encoding rules.** Use lowercase `udgt` and `udrt` in serialized amounts. Reject fractional base units. This matches the selected accounting path. It requires conversion or rejection of other formats. Do not interpret an unlabelled legacy integer as a native amount.

**PROPOSED — EC-03.** Issue the complete approved DGT total in the canonical genesis. Disable every post-genesis DGT mint path. Do not burn DGT in the initial protocol. Transfer slashed DGT into a separate penalty escrow until the approved penalty rule assigns its destination. This preserves the fixed total. Fully issued does not mean liquid, transferable, or circulating.

**MISSING — EC-03.** Explicit full-genesis issuance approval, mint authority removal, and the no-burn rule remain open. A cap check alone does not enforce fixed issuance. Governance must not obtain an ordinary parameter that changes the fixed total.

## Custody, vesting, and stake

**APPROVED — EC-05 explicit-input requirement only.** Production beneficiary vesting schedules are required explicit inputs. Do not copy historical schedules into production genesis. The [Batch 6 approval](batch-6/APPROVAL.json) does not select schedule values, release functions, custody or beneficiaries.

**PROPOSED — EC-04.** Record every base unit in exactly one custody category from [TOKEN_SUPPLY_MODEL.md](TOKEN_SUPPLY_MODEL.md). Keep allocation category, beneficiary, treasury ownership, and vesting conditions as separate metadata. A label does not lock funds.

**PROPOSED — EC-05.** Each allocation recipient must have a reviewed amount, beneficiary, control policy, lock schedule, and ownership evidence. Use a signed custody record for each recipient. Use multiple independent custodians for shared reserves and treasury operations. The signing threshold and people remain unapproved.

**PROPOSED — EC-05.** Use linear release after the cliff, with no release before the cliff. For grant `A`, approved cliff time `c`, and end time `e > c`, set vested to zero at or before `c`, to `floor(A*(t-c)/(e-c))` between `c` and `e`, and to `A` at or after `e`. Use finalized consensus time. Enforce `0 <= vested <= grant` and monotonic cumulative release. Test before, at, and after the cliff and final release. Do not copy historical durations into mainnet terms.

**APPROVED — reward eligibility for locked principal.** The [integration follow-up approval](batch-6/integration-followup/APPROVAL.json) permits locked DGT to bond and earn only when its explicit vesting policy permits staking. Otherwise reject the bond. Preserve the lock through bonding and unbonding. Count each funded bonded unit once for its owner. Exclude liquid balances, unbonding and unrelated deposits. The eligible validator must be active and not jailed in the parent-state snapshot. Actual schedules, recipients and custody records remain missing.

**PROPOSED — remaining EC-05 custody detail.** Return still-locked principal to locked custody after unbonding. The complete custody schema and production lock enforcement remain unqualified.

**MISSING — EC-05.** Recipient addresses, beneficiary identities, custody signatures, vesting amounts and dates, initial delegations, commissions, unbonding duration, penalties, and named approvers remain absent. Do not generate substitute identities or infer vesting from the five allocation categories.

## Governance dependency

**APPROVED — launch scope.** Full initial governance, staking, delegation, slashing, rewards, and treasury controls are required. The instruction does not approve the current dormant governance defaults.

**APPROVED — bonded-stake-only eligibility.** The user approved voting power from bonded stake only, excluding liquid DGT. The [Batch 5 approval record](batch-5/eligibility-followup/APPROVAL.json) records the exact question and answer. This approval does not select snapshot timing, validator eligibility, delegated-vote ownership, thresholds, deposits or timelocks.

**PROPOSED — remaining EC-10 snapshot and ownership rules.** Use one finalized, immutable eligible-stake snapshot when voting opens. Use the same snapshot for voter weight and quorum denominator. Count each bonded base unit once. Exclude unbonding funds and governance deposits. Let the beneficiary vote directly; do not also count the validator's delegated weight. Any delegated-vote fallback needs a separate override rule and tests.

**PROPOSED — EC-10.** With no eligible stake, no proposal can pass. Reject zero participation. Use checked integer cross-products for threshold comparisons. Define quorum over all snapshot stake; include abstentions for quorum and exclude them from the approval denominator. Use a veto comparison only after the participation check. Apply approved parameter changes once after a timelock. Persist deposit settlement, state changes, and events atomically.

**MISSING — EC-10.** Exact eligible-validator rules, deposit amount, voting and deposit durations, quorum, approval and veto thresholds, boundary operators, timelock, emergency authority, and parameter domains need approval. Stake-only voting reduces liquid-balance inconsistency. It does not establish resistance to concentrated ownership or capture. Model control concentration across custodians and beneficiaries before approval.

## Repair dependencies

**OBSERVED.** Batch 1 found three core genesis failures and five governance failures. The source input hash was `c78c5181af276c2f61e7553af085caacda9adb55336f4df0e749624c360107e8`. [TEST_FAILURES.json](batch-1/TEST_FAILURES.json) retains the exact errors. These are baseline failures, not fresh Batch 2 results.

| Baseline cases | Required repair order | Closure evidence |
|---|---|---|
| B1-T011 through B1-T013: genesis total, transfer permission, vesting | Apply the approved six-decimal scale; approve remaining EC-03/05 terms; isolate historical core fixtures; generate native fixtures from one versioned allocation manifest; enforce custody and locks | Exact total and five shares; before/at/after vesting boundaries; stake debit once; wrong-unit rejection; import and restart equality |
| B1-T004 through B1-T008: governance | Approve EC-10; integrate validator-backed stake; persist snapshots; reconcile tally, block hooks, execution, and events | All five failed cases; zero stake and zero participation; threshold edges; deposit conservation; duplicate execution and restart |

The approved Batch 5 eligibility follow-up closes the five historical development governance failures. Finalized snapshots, canonical eligibility, atomic settlement and the remaining mainnet governance terms still require implementation and qualification.

The six-decimal scale is approved. Historical fixtures must still identify their own units and schedules. That repair does not close mainnet genesis qualification. Never change an expected amount only to make a test pass.

## Source references

**OBSERVED.** Batch 1 preserved exact source bytes in [NODE_TEST_INPUT.tar.gz](batch-1/evidence/NODE_TEST_INPUT.tar.gz) and mapped hashes in [SOURCE_PIN.json](batch-1/evidence/SOURCE_PIN.json). Relevant archive paths are `dytallix-fast-launch/node/src/transaction_cost.rs`, `state/mod.rs`, `genesis.rs`, `runtime/staking.rs`, `runtime/governance.rs`, and `blockchain-core/src/genesis.rs` and `genesis_integration.rs`. The core genesis examples use inconsistent legacy unit assumptions and allocation labels. They are not the approved manifest.

**MISSING.** Named economics owner, independent reviewer, signed decision record, executable genesis, and release qualification remain required. See [ECONOMIC_DECISIONS.json](batch-2/ECONOMIC_DECISIONS.json).
