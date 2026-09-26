# Reward v2 genesis contract

Status: development integration. Production activation remains disabled.

## Explicit activation

A fresh native genesis can contain a `reward_v2` object. Missing input preserves the legacy development path. A supplied null or malformed object rejects. The object requires these fields:

| Field | Required input |
|---|---|
| version | 2 |
| activation_height | 1 |
| decimals | 6 |
| profile | development |
| max_validators | Explicit positive runtime limit |
| max_positions | Explicit positive runtime limit |
| validators | Array of address, active, and jailed fields |
| positions | Array of owner, validator, and amount_udgt fields |

Amounts are integer digit strings. Runtime validation bounds each resource limit to 10,000. Actual counts must fit the configured limits. A mainnet profile rejects. This development path does not establish finalized consensus.

The importer derives chain identity and genesis digest from the monetary genesis marker. The digest covers the chain ID and exact input bytes. Input cannot substitute a separate reward identity. The initial reward state has height zero and no reward liabilities. Interval activation starts at height one.

## Funding and eligibility records

Each validator has explicit active and jailed status. Duplicate validator identifiers reject. Every position references a listed validator and a funded account. Duplicate owner-validator positions and zero positions reject.

For each owner, the sum of reward positions must equal that owner's existing genesis staking delegation. The importer debits the allocation through the existing staking path once. Reward positions record ownership of that same bonded principal. They do not mint or debit principal again. Liquid DGT plus bonded DGT must equal the input allocation total.

Inactive or jailed validators can retain funded positions. Their status excludes those positions from eligible reward weight during interval processing. Validator aggregate stake does not create an additional owner entitlement.

## Vesting input

Every account in a reward v2 genesis requires explicit vesting. The supported objects are:

```json
{"kind":"unlocked"}
```

```json
{
  "kind":"linear_after_cliff",
  "total_amount":"100",
  "start_time":0,
  "cliff_duration":10,
  "vesting_duration":30,
  "allow_staking":true
}
```

These values illustrate synthetic test input. They do not define a production grant.

For a linear schedule, total_amount must equal the account's complete DGT allocation and must be positive. The final release must follow the cliff. The importer checks that the end timestamp fits in u64. It rejects unknown kinds and fields.

A scheduled account can have an initial bonded position only when allow_staking is true. The stored lock covers the original allocation, including bonded principal. An account that forbids staking can retain its full allocation without a position. The runtime must preserve this lock when principal bonds or unbonds and must enforce the remaining lock when principal transfers. Genesis tests verify stored lock data and funding. Separate lifecycle tests must verify those enforcement paths.

Without reward v2 activation, the development importer accepts only an explicit unlocked object or a missing vesting field. It rejects a supplied locked schedule. Missing development input does not satisfy production beneficiary requirements.

## Atomic initialization and restart

The importer validates the full configuration before it writes. It stores balances, legacy stake ownership records, supply counters, chain identity, the monetary marker, and `rewards:v2:state` in one synchronous database batch.

On reopen, the source marker must match. Persisted reward configuration must match the genesis-derived configuration. Live reward positions and liabilities may change after genesis; reopen does not overwrite them with initial values.

Activation over an existing legacy store rejects. A missing reward state on a v2 store rejects. A reward state without matching genesis activation rejects. No automatic migration, claim erasure, or reward-index conversion occurs.

Nonzero legacy reward index, pending emission, residual, accrued claims, or delegator reward cursor rejects on v2 reopen. Malformed legacy records also reject. These failures leave database state unchanged.

## Remaining qualification

Development validator identifiers and test schedules are synthetic. Production beneficiaries, custody records, validator keys, exact schedules, release qualification, finality evidence, and activation approval remain required. Reward-rule approval does not approve those records or authorize production activation.
