# Tokenomics

This page describes the token model of the mainnet candidate. The public
testnet is a separate chain with different fee rules; see
[Public Testnet](#public-testnet) at the end.

Decision IDs (such as D05-Q02) refer to
`mainnet/launch/MAINNET_DECISION_REGISTER.json`. Where a value is still an
open decision, this page says so.

## Token Roles

### `DGT`

- staking: DGT is bonded to validators
- governance: voting weight and proposal deposits
- fixed total: 1,000,000,000 DGT
- micro-denom: `udgt` (1 DGT = 1,000,000 uDGT)

No fee is paid in DGT, and no DGT is burned. All DGT is issued at genesis,
and nothing mints DGT later (D05-Q02, P01, 29 September 2026).

### `DRT`

- pays every transaction fee
- pays validator and staking rewards
- micro-denom: `udrt` (1 DRT = 1,000,000 uDRT)

The first users get DRT for fees from a liquid bootstrap: genesis creates
a fixed amount of ordinary, transferable DRT, counted in supply, and the
genesis manifest assigns it to named accounts such as validator operators
and custody accounts (D08-Q02, P01, 29 September 2026). The amount and
recipients are genesis inputs (D08-Q02, D08-Q03).

## Fees

- Fees are paid in uDRT by the transaction's actor. There are no tips.
- The charge is `max(measured gas, minimum gas) × gas price`. Failed and
  out-of-gas transactions pay it too.
- Every transaction fee is burned: ordinary, governance and recovery. The
  payer's uDRT and the DRT supply fall by the same amount.
- A transfer to an address with no account creates the account and burns
  the account creation fee.
- The minimum fee protects against spam. It is not a floor for validator
  income; validators are paid from issuance.
- The gas price, per-resource costs and account creation fee are set at
  genesis. Governance can change them within genesis bounds. Changing where
  fees go needs an upgrade.
- No production fee values are set yet. They are genesis inputs.

Every write carries an explicit gas limit and maximum fee in uDRT. The CLI
requires both (`--gas-limit`, `--maximum-fee-udrt`); see the
[CLI reference](cli-reference.md).

Defined in [fees v1](../../node/docs/architecture/fees-v1.md) and
[governance v1](../../node/docs/architecture/governance-v1.md).

## Issuance And Rewards

- DRT supply is the genesis supply plus issuance minus burns.
- Issuance is split 40% validator rewards, 30% staking rewards and 30%
  treasury. The rounding remainder goes to the issuance reserve.
- The validator share is divided every block by voting power among the
  active validators and credited to each operator's owner.
- The staking share is paid pro rata to bonded stake, with no commission.
- Both are claimed with `dytallix stake claim`.
- Treasury spending is POST MAINNET.

Issuance per epoch comes from the adaptive emission controller. Its only
input is an observation that every validator derives from committed blocks:
block-space utilization, with volatility 0. There is no oracle. This
observation contract is approved (D01-Q02, P01, 29 September 2026). The
controller's parameters and first command (D01-Q01) and the epoch length
(D03-Q01) are open.

Defined in [fees v1](../../node/docs/architecture/fees-v1.md) and
[adaptive emission v1](../../node/docs/mainnet/adaptive-emission-v1.md).

## Staking And Governance

Staking:

- An account bonds DGT to a validator with `dytallix stake bond` and begins
  unbonding with `dytallix stake unbond`.
- An unbond matures after the evidence age limits plus processing margins.
  Those values are open (D09-Q03).
- An unbond can be withdrawn once it matures and no penalty on it is
  unsettled (D09-Q05, P01, 30 September 2026).
- Double-signing is penalized: a validator's first duplicate vote deducts a
  fixed share from all stake bonded to it at that height, the operator's and
  every delegator's, and removes the validator for good. Later evidence
  against it adds nothing. Light-client-attack evidence is recorded only,
  and there is no downtime penalty (D09-Q04, P01, 30 September 2026). The rate is a genesis
  input.
- Vesting-locked stake carries the same risk: a penalty comes off the amount
  still locked, on the same vesting dates.
- Penalized DGT moves to a penalty escrow that nothing can spend; the DGT
  total stays fixed.

Governance:

- Voting weight is linear in stake. Each account votes with its own
  effective bonded DGT at the proposal's snapshot. Liquid, queued and
  non-effective DGT count zero.
- Votes cannot be delegated, and voting weight does not decay.
- Abstain counts for quorum only.
- Proposal deposits are held in DGT escrow and refunded at every outcome.
  No deposit is burned.
- Governance can change the ordinary fee profile, `min_self_bond`,
  `max_active` and the validator operator registry. Everything else changes
  only by a root-signed upgrade.
- Quorum, thresholds, deposit, periods and timelock are open (D11-Q02).

Defined in [governance v1](../../node/docs/architecture/governance-v1.md)
and [liveness v1](../../node/docs/architecture/liveness-v1.md).

## Whitepapers

The whitepapers describe an earlier design: vote decay, a fee split with
tips, an oracle-enforced fee floor and a liquidity bootstrapping pool. The
mainnet candidate supersedes those claims. See
[Errata for the mainnet candidate](whitepapers.md#errata-for-the-mainnet-candidate).

## Public Testnet

The public testnet at `https://dytallix.com` is a separate chain running a
different node. In April 2026 its `GET /status` reported
`fee_denom: "udgt"` and `min_gas_price: 1000`, so testnet fees are paid in
DGT. It also has a faucet; see [Getting Started](getting-started.md). Check
the testnet's `/status` before hard-coding fee assumptions for it.

None of this applies to the mainnet candidate, which charges fees in uDRT
and has no faucet.
