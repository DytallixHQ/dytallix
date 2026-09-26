> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# DRT tokenomics

**DRAFT NOT FROZEN. This document does not approve mainnet activation.**

APPROVED means the user selected the rule within the recorded scope. OBSERVED means the inspected source or evidence contains the rule. PROPOSED means an engineering recommendation awaits approval. MISSING means the required decision or evidence has not been supplied. Approval of a policy does not prove implementation.

## Issuance and allocation

**APPROVED — EC-06.** Keep the selected corrected adaptive controller. D01 records the user selection on 9 September 2026. The selected model is not the website's piecewise-linear display and not the development percentage schedule.

**OBSERVED — EC-06.** The candidate `docs/mainnet/adaptive-emission-v1.md` defines integer udrt per epoch, a finite error window, soft and hard regimes, damping, and a command for the next epoch. The controller returns a command; it does not mint by itself. The issuance timing development path stages that command with block issuance and settlement. Legacy scheduled issuance remains a separate development path. The controller's economic operating domain and production integration remain unqualified.

**MISSING — EC-06.** Approve the corrected specification version, initial state and first-epoch command, observation contract, calibrated gains, input domain, bounds, and governance migration. Complete the conditional proof obligations and independent review. A clipped output does not prove system stability. No numerical controller defaults become mainnet parameters here.

**APPROVED — EC-07.** DRT reward shares are validators 40%, stakers 30%, and treasury 30%. The [saved approved-source record](evidence/TOKENOMICS_APPROVED_SOURCE.json) supports these shares. The native 60/25/10/5 pools conflict with this policy. Additional AI or bridge reward pools cannot be added to a total that already equals 100%.

**APPROVED — EC-02 decimal scale.** Both tokens use six decimal places: `1 DRT = 1,000,000 udrt`. The [Batch 6 approval record](batch-6/APPROVAL.json) approves the scale. It does not approve initial DRT supply or issuance settings.

**MISSING — EC-11.** Initial DRT amount, recipients, custody, total-supply limit, and authorized burn policy remain undecided. Do not infer a zero initial supply, infinite supply, or a perpetual mint floor from existing defaults.

**PROPOSED — EC-11.** Fund a measured liquid DRT bootstrap at genesis. Derive the amount from the approved fee schedule, startup transactions, required operators, and recovery allowance. Allocate it through the approved custody manifest. The amount is deliberately unset. Reward pools alone cannot pay initial fees while claims reject.

## Reward settlement

**APPROVED — EC-07 epoch split.** The [issuance timing approval](batch-6/issuance-timing/APPROVAL.json) adopts `V = floor(4E/10)`, `S = floor(3E/10)` and `T = floor(3E/10)` for integer epoch command `E`. Record `R = E - V - S - T` in the separate `issuance_reserve` reserve. Then `0 <= R <= 2 udrt`. Use checked arithmetic. These four budgets sum exactly to `E`. The reserve has no automatic recipient or sweep authority. It is separate from staking-recipient rounding and inactive-period reserves.

**APPROVED — EC-08 interval allocation arithmetic.** Allocate each interval budget `B` against its supplied eligible-stake snapshot. For total eligible weight `W > 0` and recipient weight `w_i`, assign `q_i = floor(B*w_i/W)` using checked wide arithmetic. A sole eligible recipient receives all of `B`. Keep `B - sum(q_i)` in a separately recorded reward-pool reserve. This reserve has no automatic recipient or sweep authority. The [Batch 6 approval](batch-6/APPROVAL.json) covers these rules only.

**APPROVED — EC-08 interval timing and identity.** The [integration follow-up approval](batch-6/integration-followup/APPROVAL.json) adopts one finalized block per interval. Use eligible stake from the parent block's finalized state. In-block changes affect the next interval. Use chain identity, genesis digest, rule version and height to identify the interval. Record it once with finalization. Reject conflicting retry inputs.

**APPROVED — epoch-to-block budget mapping.** Set a positive epoch length `N` explicitly in finalized blocks. For each bucket budget `B`, including `issuance_reserve`, allocate `floor(B/N)` per block. Add one udrt to each of the first `B mod N` blocks. Apply this schedule separately to all four buckets. An epoch's block allocations sum exactly to its command. Old reward liabilities remain backed and do not become new interval budgets.

**APPROVED — initial and later commands.** Supply the epoch-zero command `E0` explicitly in genesis configuration. Derive each later command from the completed previous epoch's observation, bound to the expected parent state. `E0` is an issuance command, not initial liquid DRT supply. No production value for `N`, `E0` or controller parameters is selected by this approval.

**PROPOSED — EC-08.** Record each integer entitlement against a backed reward pool before a stake change. A claim moves custody from that pool to the beneficiary's liquid account. It does not mint. A claim must authenticate the beneficiary, consume the claim identifier once, and persist the debit and credit atomically. Retain the entitlement after exit or key rotation under the approved account authorization policy.

**APPROVED — EC-08 staking-pool eligibility and ownership.** Use funded bonded DGT delegated to an active, non-jailed validator in the snapshot. Exclude liquid balances, unbonding and other-purpose deposits. Credit the owner of each bonded unit once. Include validator self-stake only through that owner record. Add no validator aggregate weight and deduct no staking-pool commission. Locked principal requires explicit permission to stake and must retain its lock. These rules do not qualify the separate validator reward pool.

**APPROVED — EC-08 inactive staking intervals.** If no stake is eligible, retain the interval budget in a separate inactive-period reserve. Later entrants receive none automatically. Assign no spending authority. The [follow-up approval](batch-6/integration-followup/APPROVAL.json) adds this rule; it was not part of the original rounding-reserve approval.

**MISSING — remaining EC-07/08 terms.** Future reserve spending, separate validator-pool qualification and treasury custody remain open. Canonical validator status and stake backing require production implementation and evidence. No production validator set or custody record is approved by the eligibility rule.

**OBSERVED.** B1-T009 returned 249999 udrt when the sole-delegator fixture expected 250000. The fixed-point index has more than one rounding step. A fixture change alone cannot close this accounting question.

**PROPOSED — repair order.** Implement the approved EC-08 arithmetic and separate reserve. Specify entitlement and reserve state. Replace or reconcile the two-stage index arithmetic. Keep remaining EC-07/08 decisions open. Test a sole beneficiary, unequal weights, sub-unit shares, stake changes, zero-stake periods, exit, failed claims, duplicate claims, and restart. Verify every residual in custody. Keep the reward test separate from the approved change from legacy pool shares to 40/30/30.

**APPROVED — versioned activation contract.** Bind reward version, unit scale and eligibility parameters into approved fresh genesis. Begin at the first finalized post-genesis block. Reject unconverted legacy reward state; do not erase or reinterpret index claims. A migration needs a separate reviewed conversion plan. Production activation requires qualified finality, claims, backing, duplicate handling, bounded resources, recovery, launch rehearsals and final authorization. The current development implementation combines explicit issuance timing with the reward adapter. It does not qualify production activation or close the original legacy reward failure. Final issuance timing evidence belongs in its separate follow-up.

**OBSERVED — small-stake measurement.** [Synthetic measurements](batch-6/integration-followup/measurement/REPORT.md) confirm that per-block floors can leave positive stakes with zero payout indefinitely under fixed inputs. A synthetic 0.3 DRT staking budget and one billion eligible DGT need 3,333.333334 DGT for one base DRT per block. Neither input is a production forecast. No fractional carry, longer interval, minimum stake or reserve redistribution was added by this measurement.

## Timebase and inputs

**APPROVED — EC-09 block-count timebase.** Use explicit `N` finalized blocks per epoch. Do not infer a production value or substitute wall-clock time. The earlier 100-slot, two-second calibration candidate remains unapproved. It does not define `N` or production block duration.

**PROPOSED — EC-09.** Count finalized used capacity against the fixed scheduled capacity for the whole epoch. Empty slots contribute zero used capacity. Use a deterministic capacity definition and finalized input record. State the controller's window as an exact sample count. A timebase change requires recalibration because gains act per sample.

**APPROVED — strict development boundary.** Reject the development epoch-boundary block if the required completed-parent-epoch observation is missing, invalid or bound to the wrong parent. Generate no substitute observation or catch-up issuance. Commit no partial controller, issuance, reward, supply or block changes. This is the development rejection contract, not a production pause-and-resume policy.

**PROPOSED — production EC-06/09 handling.** If a required observation is invalid or unavailable, pause new emission commands and issuance. Do not fabricate a zero observation, use a local network response, or mint skipped commands later. Existing token transfers can continue if consensus and fee rules permit them. Because the candidate rejects skipped epoch identifiers, resumption needs an approved state migration and input rule. That transition is missing and blocks activation.

**APPROVED — atomic issuance settlement.** Stage the controller journal, command consumption, four bucket credits, reward state, supply counters and block settlement in one storage batch. Commit all changes together. A failed commit changes none of them. The development implementation follows this boundary; production finality, observation authentication and aggregation, recovery qualification and activation remain open.

## Fees and burn

**PROPOSED — EC-12.** Use DRT for fees. Separate compute charge, bandwidth charge, and explicit tip. Reserve one maximum liability before execution. Charge metered accepted work within that maximum. Refund unused liability. Require the same versioned calculation in admission and settlement. Unaccepted invalid transactions create no on-chain fee debit; accepted execution failures retain only their approved metered charge.

**PROPOSED — EC-12.** Burn the bandwidth charge from DRT custody. Pay the compute charge and tip to the eligible block reward recipient. Define `maximum_reservation = refund + compute + bandwidth + tip`. Use integer prices in udrt per metered unit. This recommended destination rule is separate from issuance shares and remains unapproved.

**PROPOSED — EC-12.** Commit fee debit, refund, recipient credit, burn counter, supply reduction, and receipt in one state transition. A failed state commit changes none of them. Do not label a diagnostic fee percentage as a token burn. Do not burn DGT to settle a DRT liability.

**MISSING — EC-12.** Meter definitions, price bounds, minimum charge, recipient eligibility, failed-execution rules, fee update limits, and governance authority need approval and tests. Current scalar gas and diagnostic burn defaults do not approve this proposal.

## Evidence and activation

**OBSERVED.** Baseline evidence uses node source SHA256 `c78c5181af276c2f61e7553af085caacda9adb55336f4df0e749624c360107e8`. The [source archive](batch-1/evidence/NODE_TEST_INPUT.tar.gz) contains `crates/adaptive-emission`, `docs/mainnet/adaptive-emission-v1.md`, `docs/mainnet/specification-decisions.md`, and `dytallix-fast-launch/node/src/{block_lifecycle.rs,execution.rs,settlement.rs,supply.rs,runtime/staking.rs,runtime/fee_burn.rs}`. [TEST_FAILURES.json](batch-1/TEST_FAILURES.json) records the failed reward case. These are Batch 1 observations, not a claim that Batch 2 has repaired the runtime.

**MISSING.** Signed economic decisions, integrated consensus and governance, complete custody settlement, economic calibration, independent review, and release qualification remain required. [TOKEN_SUPPLY_MODEL.md](TOKEN_SUPPLY_MODEL.md) defines the proposed accounting invariants.
