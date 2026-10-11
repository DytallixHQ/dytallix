# DGT tokenomics

Status: NOT FROZEN, NO GO. Current as of 10 October 2026; it freezes with the release candidate (E06), and the 30-day public review covers it. The [decision register](DECISIONS_REQUIRED.md) and the dated P01 [approvals](approvals/) are authoritative where wording differs. The September draft, with its proposals and batch history, is in this file's history.

DGT is the fixed-supply token: it stakes, secures the validator set and votes in governance. Fees and rewards are paid in DRT ([DRT tokenomics](DRT_TOKENOMICS.md)).

## Supply

- **Fixed total.** 1,000,000,000 DGT at six decimals: `1,000,000,000,000,000 udgt` ([token unit contract](../node/docs/mainnet/token-unit-contract.md)).
- **All at genesis** (D05-Q02, P01, 29 September 2026). The canonical genesis issues the whole total. Nothing mints DGT later: the runtime mint path is removed, and genesis import is the only DGT credit, capped at the total. The binding review refuses a production genesis that does not issue the whole total. No governance parameter changes it; changing the total needs an upgrade.
- **No burn.** No DGT is burned in the initial protocol. Penalized DGT goes to a penalty escrow that nothing can spend (D09-Q04, [penalties v1](../node/docs/architecture/penalties-v1.md)).
- **No extra stake.** No DGT is created for validators on top of the total: initial stake moves or encumbers existing units.

Fully issued does not mean circulating: almost all DGT starts in the founder's bucket accounts.

## Allocation at genesis

The shares come from the approved tokenomics source (EC-01). The accounts are the solo launch profile's, with the root key holders' grants ([genesis framework](GENESIS_FRAMEWORK.md#1-tokens)).

| Bucket | Share | DGT | udgt | At genesis |
|---|---:|---:|---:|---|
| Ecosystem growth | 30% | 300,000,000 | 300,000,000,000,000 | The founder's bucket account. The validator's self-bond and the public review's bug bounty come from it. |
| Team and advisors | 20% | 200,000,000 | 200,000,000,000,000 | 199,900,000 DGT in the founder's bucket account; 20,000 DGT to each of the five root key holders, unlocked |
| Public sale | 15% | 150,000,000 | 150,000,000,000,000 | The founder's bucket account |
| Private sale | 15% | 150,000,000 | 150,000,000,000,000 | The founder's bucket account |
| Reserve | 20% | 200,000,000 | 200,000,000,000,000 | The founder's bucket account |
| **Total** | **100%** | **1,000,000,000** | **1,000,000,000,000,000** | |

- **Bucket accounts.** Each is the founder's, publicly listed with its purpose, with its key in the founder's own custody, outside the root key kits ([root key holders](approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json)). A bucket's name does not lock it; every movement out of a bucket is a public on-chain transfer.
- **Holder grants.** 100,000 DGT in all (0.01% of supply), moved from Team and advisors inside the fixed total. Each holder's account key is on a separate wallet fob, never on their kit, and the accounts are listed as "kit holder 1" to "kit holder 5", never by name.
- **No vesting at launch.** Production vesting is neither approved nor built, and the native importer accepts only unlocked balances. If vesting is added later, locked DGT may bond and earn only when its vesting policy permits staking, and the lock is kept through bonding and unbonding (approved rule for locked principal).
- **Concentration.** The founder holds 99.99% of DGT at genesis. The [trust model](TRUST_MODEL.md) discloses it.

Amounts are serialized as integer strings in `udgt`; the genesis importer accepts base units only and refuses whole-token aliases, fractions and a supplied schedule it cannot enforce.

## Staking

- Owners bond DGT to a validator with ordinary transactions; the staking pool pays owners with no commission.
- At most 16 active validators (governed between 4 and 32), each with a 100,000 DGT minimum self-bond (governed between 10,000 and 1,000,000 DGT). Validator power is bonded stake ([validator lifecycle](../node/docs/architecture/validator-lifecycle-retention.md)).
- An unbond matures after the 14-day evidence age plus a 1-hour margin and is withdrawable once no penalty on it is unsettled (D09-Q03, D09-Q05).
- A validator's first duplicate vote deducts 5% from all DGT bonded to it at that height, self-bond and delegators alike, and removes it for good (D09-Q04).
- Bonded DGT earns DRT: the staking share of issuance, pro rata to bonded stake ([DRT tokenomics](DRT_TOKENOMICS.md#rewards)).

## Governance

Governance v1 ([governance v1](../node/docs/architecture/governance-v1.md), D11-Q01, D11-Q02):

- **Weight.** Bonded DGT only, at the finalized parent block, by stable account ID. Each owner votes its own bond; there is no vote delegation, validators have no special vote, and liquid DGT carries no vote.
- **Thresholds.** Quorum 33.4%, approval 50%, veto 33.4% (basis points 3,340, 5,000 and 3,340).
- **Deposit and periods.** A 10,000 DGT deposit, escrowed and refunded at every outcome; 7-day deposit and voting periods; a 2-day timelock before execution.
- **Scope.** Parameter changes (the ordinary fee profile, `min_self_bond` and `max_active`, within genesis bounds) and the validator registry. Upgrades stay root-signed only, and treasury spending is POST MAINNET.

Since the founder holds 99.99% of DGT at genesis, the founder can outvote every other holder by bonding, until DGT is distributed.

## Still open

- The bucket, treasury and holder account addresses (D08-Q01) and the treasury record (D02-Q02).
- The validator's self-bond amount (D09-Q02).
- The bug bounty tiers and amounts, paid after genesis.
- Qualification on the staging hosts: DGT transfers, staking, delegation and supply reconciliation (G13, G15, G16, G17).

Further detail: [DGT supply contract](../node/docs/mainnet/dgt-supply-contract.md), [token supply model](TOKEN_SUPPLY_MODEL.md).
