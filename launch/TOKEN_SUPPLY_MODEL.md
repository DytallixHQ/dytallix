# Token supply model

Status: NOT FROZEN, NO GO. Current as of 10 October 2026; it freezes with the release candidate (E06). This is the accounting model behind [DGT tokenomics](DGT_TOKENOMICS.md) and [DRT tokenomics](DRT_TOKENOMICS.md); the [decision register](DECISIONS_REQUIRED.md) is authoritative where wording differs. The September draft, with its proposals and batch history, is in this file's history.

## Units

Both tokens have six decimals: `1 DGT = 1,000,000 udgt` and `1 DRT = 1,000,000 udrt` ([token unit contract](../node/docs/mainnet/token-unit-contract.md)). Amounts are nonnegative integers in base units, serialized as decimal strings. Arithmetic is checked: overflow is refused, and no balance uses floating point or silent saturation.

## Custody

Each base unit belongs to exactly one custody category of its token. Ownership, bucket names and vesting are metadata, not extra balances.

| Token | Symbol | Custody category |
|---|---|---|
| DGT | Lg | Liquid accounts |
| DGT | Kg | Locked, not bonded or unbonding (none at launch: no vesting) |
| DGT | Bg | Bonded stake, including any permitted locked stake |
| DGT | Ug | Unbonding stake |
| DGT | Gg | Governance deposit escrow |
| DGT | Pg | Penalty escrow, which nothing can spend |
| DRT | Lr | Liquid accounts, including the treasury account |
| DRT | Kr | Locked accounts outside reward or fee custody (none at launch) |
| DRT | Fr | Fees withheld while a block runs; zero at every commit |
| DRT | Vr, Sr, Tr | Validator, staking and treasury reward pools, each net of its reserves |
| DRT | Xvr | Validator pool reserve: rounding, and blocks with no eligible validator |
| DRT | Xir | Issuance-split reserve |
| DRT | Xsr | Staking pool rounding reserve |
| DRT | Zr | Inactive-period staking reserve |

A mempool reservation restricts a liquid balance; it is not a second balance. Reward entitlements are liabilities backed by `Vr` and `Sr`, never added to supply again. The controller's uncommitted command is neither issuance nor custody. A monetary transition that cannot map to this partition is refused; new custody needs a schema migration and new invariants.

## Conservation

At every committed height `h`, in base units:

```text
Cg(h) = Lg + Kg + Bg + Ug + Gg + Pg
Cg(h) = Ig + Mg(h) - Dg(h)

Cr(h) = Lr + Kr + Fr + Vr + Sr + Tr + Xvr + Xir + Xsr + Zr
Cr(h) = Ir + Mr(h) - Dr(h)
```

`I` is genesis-issued supply, `M` cumulative post-genesis issuance and `D` cumulative destruction. A transfer, lock, bond, unbond, deposit or reward claim leaves supply unchanged. Burn counters are history, not live custody.

| Term | Value | Source |
|---|---|---|
| `Ig` | `10^15` udgt, the whole fixed total | D05-Q02 |
| `Mg`, `Dg` | 0: no DGT mint and no DGT burn | D05-Q02; penalties go to `Pg` |
| `Ir` | `10^12` udrt, the 1,000,000 DRT bootstrap | D08-Q02, [DRT supply](approvals/P01_E05_DRT_SUPPLY_2026-10-10.json) |
| `Mr` | The controller's issuance, 500 to 1,000 DRT a block | D01-Q01 |
| `Dr` | Every fee, burned at the end of its block | D05-Q01, [fees v1](../node/docs/architecture/fees-v1.md) |

DRT has no supply cap. Genesis bucket totals and genesis custody totals must each equal `Ig` independently: the buckets partition beneficiaries and the categories partition state, so the two views are never added together.

## Transitions

```text
epoch E = Σ(block validator + staker + treasury + issuance reserve credits)
pool budget B = Σ(new integer entitlements) + pool rounding credit
fee charge: payer Lr debit = Fr credit; at block end Fr debit = Dr increase
claim: pool debit = owner liquid credit
bond: liquid (or permitted locked) debit = bonded credit
unbond: bonded debit = unbonding credit
withdraw: unbonding debit = liquid credit (+ still-locked credit)
governance deposit: liquid debit = escrow credit; refund reverses it
penalty: bonded or unbonding debit = penalty escrow credit
```

The pool budget is a block's new allocation, not all funds in a pool that also backs older entitlements; recording entitlements does not credit the pool again, and a pool-to-pool move does not mint. A burn debits custody and raises the destruction counter by the same amount. For each reward pool, held plus claimed equals issued.

## Atomic commit

Each block commits the controller journal, epoch and block identity, command consumption, the four bucket credits, reward state, supply counters and block settlement in one storage batch. A duplicate command cannot mint again; a refused or failed commit leaves all state unchanged; replay and restart preserve custody, counters and receipts. The node's supply checks enforce these equations per pool and at every commit, and the monitor job raises a severity 1 alert on a supply invariant violation ([monitoring v1](../node/docs/architecture/monitoring-v1.md)).

The epoch observation is derived from committed blocks by every validator (D01-Q02), so there is no substitute input and no catch-up issuance. Governance changes only the approved parameters within genesis bounds, after its timelock; it cannot rewrite reward entitlements, create custody or change the DGT total.

## Integer examples

Arithmetic illustrations, not genesis parameters:

| Case | Result |
|---|---|
| Issuance `E` = 1 udrt | V 0, S 0, T 0, issuance reserve 1 |
| Issuance `E` = 7 udrt | V 2, S 2, T 2, issuance reserve 1 |
| Issuance `E` = 10 udrt | V 4, S 3, T 3, issuance reserve 0 |
| Pool `B` = 250,000 udrt, one eligible owner | Entitlement 250,000; remainder 0 |
| Pool `B` = 7 udrt, eligible weights 1 and 2 | Entitlements 2 and 4; remainder 1 to the reserve |
| Pool `B` = 7 udrt, no eligible weight | Inactive-period reserve 7 |
| Fee charge 1,000,000 udrt (a basic Send) | Payer debited 1,000,000; burned at block end; supply falls by 1,000,000 |

## Still open

Qualification on the staging hosts: independent reconstruction of every custody sum against the supply counters, bound to the source, genesis and release digests, through the seven simulation runs (G05, G14, G15).
