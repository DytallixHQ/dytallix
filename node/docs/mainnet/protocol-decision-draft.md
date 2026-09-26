# Protocol decisions for approval

Status: D01 model selected on 9 September 2026. D02 reward shares and DGT allocation shares are approved as recorded below. D07 required scope is fixed by the launch brief. Other decisions remain draft.
This file does not approve mainnet or change runtime rules.
The instruction to continue engineering did not select economic parameters or a
consensus protocol. Record the existing approvals and obtain decisions for the unresolved rules before implementing
those state transitions. Do not infer approval from current defaults.

## Decisions

| ID | Concrete choice | Required decision record |
|---|---|---|
| D01: Emissions | Selected: the corrected whitepaper adaptive controller. Scheduled issuance is not the mainnet target. | Model version, issuance units, initial state, limits, parameter table, effective height, and migration. The controller also needs plant assumptions, delays, stable operating region, and failure behavior. |
| D02: Allocation | Approved allocation: validators 40%, stakers 30%, treasury 30%, from the user-designated tokenomics page. Native 60/25/10/5 conflicts with this policy. | Exact shares, recipient definitions, integer rounding, residual ownership, and zero-stake treatment. |
| D03: Timebase | Current per-block schedule or paper epochs of 100 slots at 2 seconds each. | One timebase for emissions, rewards, finality, unbonding, governance, and missing blocks. A 200-second epoch gives 3,024 epochs per week. |
| D04: Fees | Scalar gas or separate compute and bandwidth prices plus tip. | Fee token, price units, amount formula, minimum charge, reservation, failed-transaction charge, refund, and recipient. Recommend DRT as the fee token for consistency with the paper and current execution; this recommendation remains unapproved. |
| D05: Burning | Percentage of the fee or bandwidth-only burn. | Exact burn amount, proposer payment, supply reduction, update order, and rounding. Define behavior for failed execution. |
| D06: Consensus | Specify the distributed protocol for the intended stake-based chain. | Proposer selection, authenticated voting, fork choice, finality, synchronization, fault/timing assumptions, validator changes, slashing, and persistent state. A local block timer is insufficient. |
| D07: Launch scope | The launch brief requires full initial staking, delegation, rewards, fees, slashing, governance, treasury controls, wallets, and PQC. | Implement and qualify every required function. Select optional contracts, oracle, bridge, and gateway features separately. Identify every external input required by D01. |

## Proposed implementation boundary

Use a versioned fee interface shared by admission and execution. The interface
must return the same fee denomination and maximum liability for the same
transaction. Execution must settle that reservation through the approved fee,
burn, and refund rules. Test admission through execution with one account state.

Keep emission policy separate from supply accounting. Supply accounting must
recognize each mint and burn exactly once. An emission policy proposes an amount;
it must not bypass supply limits, authorization, or deterministic arithmetic.

Keep consensus decisions separate from node services and transport. Define the
persistent consensus state and deterministic transition inputs before choosing
module interfaces. Ordinary module extraction does not implement finality.

Do not defer functions required by the launch brief. Defer optional bridge, oracle,
and other extensions only when the approved consensus and economic design do not require them.
Implement D01 through the adaptive policy module. Keep the module inactive until
its specification, calibrated proof domain, and accounting integration pass review.

## Acceptance record

For each row record: selected option, exact values and units, specification
version, rationale, protocol owner, independent reviewer, approval date,
activation/migration rule, and required test vectors. Assign owners before
estimating a launch date.

No row in this draft closes the mathematical review, security review, or launch
qualification gates.


## Batch 1 requirement reconciliation: 9 September 2026

The user identified https://dytallix.com/developers/tokenomics as the approved mainnet allocation source. The live page was inspected in the mainnet launch task. Its fixed DGT supply is 1,000,000,000 DGT. Approved initial allocation shares are ecosystem growth 30%, team and advisors 20%, public sale 15%, private sale 15%, and reserve 20%. Approved DRT reward shares are validators 40%, stakers 30%, and treasury 30%.

This approval supersedes conflicting testnet allocation and reward-share defaults. Beneficiary accounts, custody, vesting execution, initial stake, initial DRT fee funding, commissions, and integer residual ownership remain incomplete. Allocation labels do not impose locks. No runtime token rules changed in this reconciliation.

The page displays a piecewise-linear DRT emission curve. The earlier decision selected the corrected adaptive controller. The user's allocation approval does not replace that controller decision. Keep this source/document conflict open until the controller specification and public description agree.

The current launch brief requires full initial staking, delegation, rewards, fees, slashing, governance, treasury controls, wallets, and PQC. These required functions cannot be deferred to meet the schedule. There is one permanent canonical mainnet genesis.

Week four requires seven full simulated mainnet launches. The older three-run and 30-day proposals are superseded for this program. Use `launch-rehearsals.json` and `launch-rehearsals.md` for the required run mapping. Local prerequisite checks do not count as full launches. The final run includes the T-minus 6-hour preflight and the T-plus 24-hour reconciliation.

Source evidence and allocation records are in `/Users/rickglenn/Developer/Dytallix-mainnet-launch/evidence/TOKENOMICS_APPROVED_SOURCE.json` and `/Users/rickglenn/Developer/Dytallix-mainnet-launch/GENESIS_ALLOCATION_DRAFT.json`. These are local evidence paths, not portable release references. Publish versioned references before release qualification.
