> Decision status: use [the current decision register](DECISIONS_REQUIRED.md). This technical draft contains historical proposals and implementation statements. The current register supersedes conflicting status statements. Original text is preserved in [the prior records](decision-register/evidence/prior-launch-records/).

# Genesis allocation review draft

**High-level allocations: USER_APPROVED_SOURCE. Executable genesis: incomplete. Not valid for mainnet. Not an importable genesis file.**

The [JSON draft](/Users/rickglenn/Developer/Dytallix-mainnet-launch/GENESIS_ALLOCATION_DRAFT.json) records approved DGT category amounts and DRT reward shares. Recipient identities, beneficiaries, custody, vesting, DRT genesis supply, and release approval remain `null`. It contains no copied testnet recipient addresses or keys.

## Approved DGT allocation

The user states that approved mainnet allocations are at [Dytallix Tokenomics](https://dytallix.com/developers/tokenomics). The parent agent verified the live Chrome page, titled “Tokenomics | Dytallix.” This approval covers the fixed DGT total, DGT category shares, and DRT reward shares. It does not identify recipients or approve launch activation.

| Approved category | Share | Approved udgt at six decimals | Approved DGT |
|---|---:|---:|---:|
| Ecosystem growth | 30% | 300000000000000 | 300,000,000 |
| Team and advisors | 20% | 200000000000000 | 200,000,000 |
| Public sale | 15% | 150000000000000 | 150,000,000 |
| Private sale | 15% | 150000000000000 | 150,000,000 |
| Reserve | 20% | 200000000000000 | 200,000,000 |
| Total | 100% | 1000000000000000 | 1,000,000,000 |

JSON monetary values use decimal strings with explicit units. Batch 6 approved six decimals. One billion DGT equals `1000000000000000` udgt. Full issuance at genesis, beneficiaries and custody remain open.

The approved human-token total matches the observed native cap when converted at six decimals. Native genesis currently checks that issuance does not exceed the cap; it does not require full issuance: [cap](/Users/rickglenn/Developer/Dytallix-mainnet-launch/snapshots/dytallix-node/dytallix-fast-launch/node/src/state/mod.rs:6), [genesis check](/Users/rickglenn/Developer/Dytallix-mainnet-launch/snapshots/dytallix-node/dytallix-fast-launch/node/src/genesis.rs:129).

The earlier 40/25/15/10/10 split is a superseded testnet candidate. It is not the mainnet allocation: [testnet source](/Users/rickglenn/Developer/Dytallix-mainnet-launch/snapshots/dytallix-node/deploy/genesis.dyt-local-1.json:3).

Bucket labels do not enforce locks. “Reserve” does not create a lock. “Team and advisors” does not create vesting. Approve each recipient, custody rule, lock implementation, vesting schedule, and stake mapping separately. The existing testnet file has no actual delegations: [source](/Users/rickglenn/Developer/Dytallix-mainnet-launch/snapshots/dytallix-node/deploy/genesis.dyt-local-1.json:11).

## DRT and adaptive issuance

DRT initial supply remains unapproved and `null`. No DRT genesis allocation or recipient is proposed. Approved DRT reward shares are validators 40%, stakers 30%, and treasury 30%. Owner staking eligibility, zero staking-pool commission, reward rounding and reserves are approved. Separate validator-pool rules, treasury records and production qualification remain open. Source: [Dytallix Tokenomics](https://dytallix.com/developers/tokenomics).

Liquid DRT fee funding remains open. Later local reward integration and claim recovery supersede the original snapshot claim limitation. See [issuance timing](batch-6/issuance-timing/REPORT.md) and [claim recovery](batch-6/claim-recovery/REPORT.md).

The corrected adaptive controller remains selected. Epoch and block issuance mapping is approved and locally integrated. Production parameters, numeric epoch length, observations and activation remain open. Allocation approval does not approve a different controller.

The website displays `min(1000 + max(utilization - 50, 0) * 80, 5000)` DRT per block. This differs from D01. The latest user statement approves allocations, not that controller replacement. Preserve D01 and resolve the discrepancy before activation. Source: [Dytallix Tokenomics](https://dytallix.com/developers/tokenomics).

## Required reconciliation

Use exact integer base units. Count each custody balance once. Separate liquid, locked, staked, unbonding, governance escrow, and other custody. These equations define required approval checks; current source does not implement every category.

```text
DGT_genesis_issued = Σ initial_liquid + Σ initial_locked + Σ initial_staked
DGT_total(t) = DGT_genesis_issued + DGT_post_genesis_minted(t) - DGT_burned(t)
DGT_total(t) = liquid + locked + staked + unbonding + governance_escrow + other_custody

DRT_total(t) = DRT_genesis_issued + DRT_minted(t) - DRT_burned(t)
DRT_total(t) = liquid + locked + withheld_fees + emission_pools + other_custody

epoch_issuance = Σ epoch_pool_credits + epoch_rounding_residual_credit
fee_debit = fee_burn + Σ fee_recipient_credits + retained_fee_custody_credit
reward_pool_debit = Σ reward_recipient_credits + reward_residual_custody_credit
```

Claimable rewards remain liabilities within pool custody until payout. Do not add them again to supply. A payout transfers existing custody unless the approved protocol explicitly recognizes new issuance. Burn counters must correspond to destroyed custody. Diagnostic counters do not establish a burn.

Approved DGT category amounts sum to one billion DGT and `1000000000000000` udgt, with zero category remainder. Recipient allocations remain open. Approved reward and emission rounding retain separate reserves without automatic recipients or sweep authority. Fee rounding remains open. See D02–D05 in the [current register](DECISIONS_REQUIRED.md).

## Decisions and approval ledger

Implement the approved fixed DGT total and category shares. Resolve DGT issuance enforcement, DRT supply constraints, mint authority, burn authority, burn formula, controller parameters, and fee distribution. Record supply-changing transitions separately from custody transfers. Detailed control approvals remain `null`.

Before executable genesis approval, record:

- Manifest version and digest, chain ID, and genesis time.
- Approved supply per denomination and exact allocation sum.
- Recipient and beneficiary records, ownership evidence, and custody policy.
- Vesting terms, lock enforcement, and initial delegation mapping.
- Liquid DRT fee funding and its authorized recipients.
- Adaptive specification version, parameters, timebase, allocation, and integration evidence.
- Mint and burn rules, authority limits, and rounding ownership.
- Independent reconciliation, approver roles, signatures, and approval time.

Each ledger entry needs an identifier, protocol version, chain ID, genesis digest, block height, transaction or epoch reference, denomination, transition type, source custody, destination custody, base-unit amount, supply delta, residual amount, authorization reference, commit status, and approval reference.

Validation performed: JSON parsed successfully; approved DGT category amounts sum to one billion DGT; their approved six-decimal conversion sums to `1000000000000000` udgt; DGT shares and DRT reward shares each sum to 10,000 basis points. No testnet recipient addresses were copied. Arithmetic validation does not complete custody approval or establish mainnet readiness.
