# Mainnet specification decisions

Status: draft. No mainnet release is approved by this document.

The 8 September 2026 review compared the supplied Technical White Paper and
Tokenomics Paper with eight DytallixHQ repository snapshots. The papers define
different economic rules from the selected fast node.

## Decisions required before protocol changes

| ID | Decision | Current node | Paper target | Status |
|---|---|---|---|---|
| D01 | Emission model | Static, phased, or percentage schedules | Corrected whitepaper adaptive controller | Selected by user on 9 September 2026; corrected specification, proof, and deterministic implementation required |
| D02 | Emission allocation | 60/25/10/5 default pools | 40/30/30 | Approved by user-designated tokenomics page: validators 40%, stakers 30%, treasury 30%; recipient, commission, and rounding rules remain open |
| D03 | Timebase | Per-block issuance; 15-second default block interval | 100 slots of 2 seconds per epoch | Unresolved |
| D04 | Fees | Scalar gas price | Separate compute and bandwidth prices plus tip | Unresolved |
| D05 | Burn settlement | Percentage tracking of total fee | Bandwidth charge reduces DRT supply | Depends on D04 |
| D06 | Finality and stake lifecycle | Fast-node local production | Distributed stake-weighted protocol | State machine required |
| D07 | Launch modules | Staking and governance writes remain incomplete | Full initial staking, delegation, rewards, fees, slashing, governance, treasury controls, wallets, and PQC are required | Required scope fixed by launch brief; implementation and qualification remain open |

Do not infer a decision from a default value. Approve the selected behavior,
units, parameter domain, migration rule, and test evidence for each row.

## Mathematical corrections

These corrections do not depend on the implementation choice:

- For a fixed total stake S, two quorums with weight greater than 2S/3 overlap
  by more than S/3. This is not an exact monetary-loss formula.
- At 200 seconds per epoch, 1,008 epochs equal 56 hours. One week contains
  3,024 epochs. A sum from t−W through t includes W+1 observations.
- Define the soft controller regime by |e| < delta and the hard regime by
  |e| >= delta. Error thresholds do not define admissible controller gains.
- Clipping emissions proves an output bound only. Define the plant, delay,
  initial state, gain domain, and switching rules before claiming stability.
- A lower bound on P_base does not bound the exponential charged price from
  below. Decide whether the floor applies to P_base or the charged price.
- Define net DRT supply as initial supply plus recognized minting minus
  recognized burning. Transfers do not change supply. Account for pending
  rewards and rounding residuals separately.
- Settle accrued rewards before changing stake. Specify zero-stake periods,
  claim checkpoints, integer rounding, and residual ownership.
- Define unchanged-state cases and emergency precedence in the algorithm
  registry transition rule.
- Regenerate all plots from the approved equations and parameter table.

## Required evidence

Each approved rule must identify its implementation, test vectors, owner, and
independent reviewer. Consensus rules must use deterministic arithmetic and
serialization. A bounded simulation is not a proof over all admitted inputs.

The PID proof remains open. A permitted proportional special case has local
state derivative [[0.465, 0.25], [-8, 0]] and eigenvalue magnitude sqrt(2).
This shows that the printed bounds alone do not guarantee local stability.
The example uses illustrative paper-model parameters, not deployed parameters.

Do not enable adaptive issuance until the corrected model, calibrated parameter
domain, deterministic reference vectors, and accounting invariants pass review.
Do not launch the intended stake-based chain before its consensus and stake
lifecycle are integrated and independently qualified.

## Acceptance order

1. Record the existing D01, D02, and D07 decisions. Approve the remaining rules and the corresponding specification version.
2. Complete the required mathematical and cryptographic review.
3. Implement one selected transition system and its migration rules.
4. Pass component tests and selected-node integration tests.
5. Complete independent security review and fix verification.
6. Qualify the frozen release with independent validators and recovery drills.
7. Approve the genesis digest, release digests, and launch decision.

## D01 decision: 9 September 2026

The user selected the corrected whitepaper adaptive model for mainnet and
requested its proof and implementation. Scheduled issuance is not the selected
mainnet target. Existing scheduled-emission tests cover development behavior only.

This decision does not approve the paper as printed, numerical controller gains,
plant assumptions, allocation, timebase, fee rules, or launch activation. Complete
the corrected state-transition specification and proof over its stated operating
domain. Map that specification to deterministic reference vectors and the selected
node. Keep the adaptive controller inactive until these requirements pass.


## Batch 1 requirement reconciliation: 9 September 2026

The user identified https://dytallix.com/developers/tokenomics as the approved mainnet allocation source. The live page was inspected in the mainnet launch task. Its fixed DGT supply is 1,000,000,000 DGT. Approved initial allocation shares are ecosystem growth 30%, team and advisors 20%, public sale 15%, private sale 15%, and reserve 20%. Approved DRT reward shares are validators 40%, stakers 30%, and treasury 30%.

This approval supersedes conflicting testnet allocation and reward-share defaults. Beneficiary accounts, custody, vesting execution, initial stake, initial DRT fee funding, commissions, and integer residual ownership remain incomplete. Allocation labels do not impose locks. No runtime token rules changed in this reconciliation.

The page displays a piecewise-linear DRT emission curve. The earlier decision selected the corrected adaptive controller. The user's allocation approval does not replace that controller decision. Keep this source/document conflict open until the controller specification and public description agree.

The current launch brief requires full initial staking, delegation, rewards, fees, slashing, governance, treasury controls, wallets, and PQC. These required functions cannot be deferred to meet the schedule. There is one permanent canonical mainnet genesis.

Week four requires seven full simulated mainnet launches. The older three-run and 30-day proposals are superseded for this program. Use `launch-rehearsals.json` and `launch-rehearsals.md` for the required run mapping. Local prerequisite checks do not count as full launches. The final run includes the T-minus 6-hour preflight and the T-plus 24-hour reconciliation.

Source evidence and allocation records are in `/Users/rickglenn/Developer/Dytallix-mainnet-launch/evidence/TOKENOMICS_APPROVED_SOURCE.json` and `/Users/rickglenn/Developer/Dytallix-mainnet-launch/GENESIS_ALLOCATION_DRAFT.json`. These are local evidence paths, not portable release references. Publish versioned references before release qualification.
