# DRT tokenomics

Status: NOT FROZEN, NO GO. Current as of 10 October 2026; it freezes with the release candidate (E06), and the 30-day public review covers it. The [decision register](DECISIONS_REQUIRED.md) and the dated P01 [approvals](approvals/) are authoritative where wording differs. The September draft, with its proposals and batch history, is in this file's history.

DRT is the fee and reward token. Every fee is paid in DRT and burned; DRT is issued to validators, stakers and the treasury each block. DGT is the fixed-supply staking and governance token ([DGT tokenomics](DGT_TOKENOMICS.md)).

## Supply

- **Six decimals:** `1 DRT = 1,000,000 udrt` ([token unit contract](../node/docs/mainnet/token-unit-contract.md)).
- **Bootstrap** (D08-Q02). Genesis creates 1,000,000 DRT of ordinary, transferable DRT, counted in supply and assigned to named accounts in the genesis manifest, so the first fees, bonds and claims can be paid: reward pools alone cannot pay them, since bonding and claiming are themselves paid transactions. The amount was approved on 30 September 2026 and confirmed on 10 October; the per-account rows are open (D08-Q03).
- **No cap** ([DRT supply](approvals/P01_E05_DRT_SUPPLY_2026-10-10.json)). Supply is the bootstrap plus emitted minus burned. Issuance stays between 500 and 1,000 DRT a block and every transaction fee is burned, which offsets it. Changing this needs an upgrade.

## Issuance

The selected corrected adaptive controller ([adaptive emission v1](../node/docs/mainnet/adaptive-emission-v1.md), D01-Q01) sets each epoch's issuance command from the previous epoch's utilization. The controller returns a command; it does not mint by itself.

| Setting | Value |
|---|---|
| Epoch | 17,280 finalized blocks, about a day at 5-second blocks (D03-Q01) |
| Base and ceiling | 1,000 DRT a block (17,280,000 DRT an epoch, about 6.31 billion DRT a year) |
| Floor | 500 DRT a block |
| Utilization target | 50% |
| First command (`E0`) | 17,280,000 DRT, equal to the base, set in genesis |
| Response | One-day linear: window 1, proportional gain 17,280,000,000,000 in both regimes, integral and derivative 0, integral limits 0, shock and volatility thresholds 1,000,000 ppm |

Issuance stays at 1,000 DRT a block up to 50% utilization and falls linearly to 500 at full blocks, so launch issuance equals the published base and never exceeds it. The gain is rescaled if the capacity tests (T05) show blocks cannot fill completely ([E05 values](E05_VALUES.json)).

**Observation** (D01-Q02, observation contract v1). Every validator derives the epoch's observation from committed blocks: the epoch's transaction bytes, excluding its own observation, over `epoch_blocks × max_block_bytes`, in parts per million; volatility is 0. CheckTx refuses submitted observations, the proposer inserts the derived one, and execution rejects a mismatch. There is no external oracle, so no input can be missing.

**Commands.** `E0` is an issuance command, not liquid supply. Each later command derives from the completed previous epoch's observation, bound to the expected parent state.

## Rewards

**Epoch split** (EC-07). For epoch command `E`: validators `V = floor(4E/10)`, stakers `S = floor(3E/10)`, treasury `T = floor(3E/10)`, and the remainder `R = E − V − S − T` (0 to 2 udrt) to the separate issuance reserve, with checked arithmetic. No AI or bridge pools exist.

**Per block.** Each bucket's epoch budget `B` is paid as `floor(B/N)` a block, with one more udrt in each of the first `B mod N` blocks, so an epoch's block allocations sum exactly to its command. Issuance, reward state, supply counters and block settlement commit in one storage batch; a failed commit changes none of them.

**Validator pool** (D02-Q01, [fees v1](../node/docs/architecture/fees-v1.md)). Each block's validator budget is divided by voting power among the active validators at block start and credited to each operator's owner. The rounding remainder, and the budget of a block with no eligible validator, stay in the pool's reserve.

**Staking pool** (EC-08). Each block is one interval, using eligible stake from the parent block's finalized state. Eligible stake is funded bonded DGT on an active validator; liquid balances, unbonding DGT and deposits are excluded, each bonded unit counts once for its owner, and there is no commission. For budget `B`, total weight `W > 0` and owner weight `w_i`, the owner gets `floor(B × w_i / W)`; a sole eligible owner gets all of `B`; the remainder stays in the pool's reserve. If no stake is eligible, the interval's budget goes to a separate inactive-period reserve that later entrants do not receive.

**Claims.** One `RewardClaim` pays an owner's staking and validator rewards from the backed pools; a claim moves DRT from a pool to the owner's balance and mints nothing. For each pool, held plus claimed equals issued. Per-block floors can leave a very small stake with no payout; no carry or minimum stake was added.

**Treasury.** The treasury share accrues to the treasury pool; the treasury account is the founder's, and spending is POST MAINNET (D07-Q01). The reserves have no recipient and no sweep authority.

**Activation.** Reward version, unit scale and eligibility parameters are bound into the fresh genesis, and rewards begin at the first finalized block after it. No legacy reward state is converted.

## Fees and burn

Fees v1 (P01, 27 September 2026; D04, D05-Q01):

- Fees are paid in udrt by the transaction's actor, with no tips.
- The charge is `max(measured gas, minimum gas) × gas price`, one price on a combined gas total, paid also by failed and out-of-gas transactions.
- Gas price 10 and minimum gas 100,000 set a 1 DRT floor; a basic Send uses about 47,600 gas and pays the floor. Per-resource costs: overhead 10,000, receipt 1,000, wire byte 2, read byte 0, write byte 1, signature or proof 20,000, action 5,000. Governance may change the fee profile within genesis bounds that keep a reference basic Send between 0.1 and 10 DRT, with the gas price between 1 and 100.
- Creating an account costs 10 DRT, governed between 1 and 100 DRT.
- Every fee, ordinary, governance, sponsored recovery and account creation, is burned: fees collect while a block runs and move to `supply:drt_burned` at its end, so the withheld counter is zero at every commit.
- No fee goes to a validator or proposer, and no fee is paid in DGT. Changing where fees go needs an upgrade.

## Still open

- The bootstrap rows (D08-Q03), including whether the root key holders' accounts receive a share.
- Qualification on the staging hosts: DRT transfers, issuance, rewards, claims, fees and supply reconciliation (G12, G14, G19, G20).

Further detail: [DRT supply contract](../node/docs/mainnet/drt-supply-contract.md), [issuance timing contract](../node/docs/mainnet/issuance-timing-contract.md), [token supply model](TOKEN_SUPPLY_MODEL.md).
