# Fee destination and validator rewards (E04)

Status: approved (P01, 27 September 2026: fee option A, validator option A);
FE1 to FE3 done. Engineering task E04. Paths are relative to
`dytallix-fast-launch/node/src/`. Decision IDs refer to
`mainnet/launch/MAINNET_DECISION_REGISTER.json`.

This step decides where collected fees go and how the validator share of
issuance is paid. It sets no production numbers: prices, the creation fee
and issuance values are E05 inputs.

## Already decided

- Fees are paid in uDRT by the transaction's actor, with no tips (AS01–AS06,
  OF01–OF05, RF01–RF06; D04-Q01 and D04-Q02 partly approved).
- The charge is `max(measured gas, minimum gas) × gas price`, paid also by
  failed and out-of-gas transactions.
- The account creation fee is burned (P01, 26 September 2026).
- Governance sets fee values within genesis bounds; changing where fees go
  needs an upgrade (P01, 27 September 2026).
- Issuance is split 40% validator rewards, 30% staking rewards, 30%
  treasury, remainder to the issuance reserve (EC-07). Treasury spending is
  POST MAINNET.
- No DGT fees and no DGT burn.

## Problems

| ID | Problem | Where |
| --- | --- | --- |
| F1 | Every transaction fee (ordinary, governance, sponsored recovery) goes into `execution:v1:withheld_udrt`, a counter with no owner that nothing ever reduces. The DRT is unspendable forever, yet it still counts in total supply. OF05 (11 September) made this a placeholder: "no recipient, burn, sweep". The launch requirements say fees "must reconcile against collection, distribution, and burning logic"; gate G20 is blocked on it (D05-Q01 open). | `settlement.rs:28, 575`; `supply.rs:578` |
| F2 | Validators are never paid their 40% of issuance. The `validator_rewards` pool only grows: nothing moves it to a validator. Only the staking pool pays out, pro rata to bonded stake, so a validator earns only on its own bond, like any staker (D02-Q01 open). | `block_lifecycle.rs:295`; `supply.rs:216` |
| F3 | The legacy `runtime/fee_burn.rs` records a 25% burn in uDGT. Consensus never calls it, but it contradicts "no DGT burn". | `runtime/fee_burn.rs` |

The treasury and issuance reserve pools also only grow; that is expected
while treasury spending is POST MAINNET.

## Options

**Fee destination (F1, D05-Q01).**

- **A. Burn every fee.** The charge reduces DRT supply (`supply:drt_burned`),
  as the creation fee already does. No recipient records or custody term;
  validators are paid from issuance. Fees offset issuance under load. The
  minimum fee then protects against spam, not validator income (resolves
  conflict AC-002).
- **B. Split.** Burn a share and pay the rest to validators by voting power.
  Adds a ratio (an E05 value) and recipient records.
- **C. All to validators**, by voting power, through the validator pool.
- **D. Keep withholding** (today). Fees stay locked but not counted as
  burned.

A burn of the bandwidth part only (proposal EC-12) is not offered: the
approved meter charges one price on a combined gas total, so it has no
separate bandwidth charge to burn.

**Validator rewards (F2, D02-Q01).**

- **A. By voting power, every block.** Each block's validator budget is
  divided among the active validators by power and credited to each
  operator's owner, claimed with the existing `RewardClaim`. A validator
  that is not in the set earns nothing for that block; the rounding
  remainder stays in the pool.
- **B. To the block proposer.** Needs the proposer's identity from the
  engine and pays unevenly.
- **C. Keep accruing (POST MAINNET).** Validators earn only on their bond
  until an upgrade.

## Rules

1. A transaction fee is burned when charged: the payer's uDRT falls and
   `supply:drt_burned` rises by the charge, in the same place
   `charge_sponsored` credits the withheld counter today. The withheld
   counter stays zero; supply is `genesis + emitted − burned`.
2. At each block start, the block's `validator_rewards` credit is divided by
   voting power among the validators in the effective set, with the same
   floor-and-remainder rule as the staking pool, and added to each owner's
   claimable rewards. The pool keeps what is unclaimed; supply checks
   `held + claimed = issued` for it, as for staking.
3. Remove `runtime/fee_burn.rs` from the consensus build's reachable code, or
   leave it with the legacy paths only.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| FE1 (done) | Burn every fee at the end of its block | Supply falls by fees |
| FE2 (done) | Validator pool allocation by power at block start; claims pay both pools; supply rule for the pool | Validator payouts |
| FE3 (done) | Tests | Tests |

**Implementation notes.**

- Fees still collect in `execution:v1:withheld_udrt` while a block runs, so
  every per-transaction reconciliation is unchanged. At the end of the block,
  after the creation-fee check, the whole amount moves to
  `supply:drt_burned` (`Settlement::burn_withheld_fees`). The counter is zero
  at every commit; the complete check refuses a state where it is not. This
  covers ordinary, governance, recovery and legacy signed fees.
- The reward state gains `validator_payouts` (owed amounts, a reserve for
  rounding and for blocks with no eligible validator, and running totals).
  At block start, after the lifecycle advances, the block's validator credit
  is divided by `LifecycleState::payout_weights`: each effective validator's
  power, summed per operator owner, skipping jailed or inactive validators.
  The staking pool's floor-and-remainder rule is reused.
- `RewardClaim` pays staking rewards from the staking pool and validator
  payouts from the validator pool. An owner who is still owed a payout keeps
  their reward slot.
- Supply: validator pool held plus claimed equals its issuance, and the pool
  equals the owed payouts plus the reserve.
- Without a lifecycle there is no voting power, so the validator share stays
  in the reserve (local profiles only).
- `runtime/fee_burn.rs` is reachable only from the legacy, non-consensus
  paths; it is left there.

## Tests

- Validator payouts follow voting power, sum per owner, skip a jailed
  validator, keep the rounding remainder and keep a slot while owed.
- Through the engine: the operator is owed the whole validator share; a
  claim pays both pools; the claim's fee is burned; the withheld counter is
  zero; a restart passes the complete check; a state that still withholds a
  fee is refused even when the amount is conserved.
- Existing fee tests now check the burn counter. Skipping the burn or the
  payout allocation fails over 100 consensus tests each.

## Decisions (P01, approved 27 September 2026)

1. Fee destination: A. Every transaction fee is burned when charged.
2. Validator rewards: A. The validator share of issuance is divided every
   block by voting power among the active validators and credited to each
   operator's owner.

## Also found

- `mainnet/launch/MAINNET_DECISION_REGISTER.json` does not yet record the
  creation-fee burn (26 September) or governance as fee authority
  (27 September).
- `docs/mainnet/drt-supply-contract.md` still says there is no burn
  transition; the creation-fee burn exists.
