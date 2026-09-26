# Genesis integration fixture contract

Status: local test fixture. This file does not approve a production genesis.

`blockchain-core/src/genesis_integration.rs` uses an independent synthetic configuration. It does not copy `GenesisConfig::mainnet()`.

## Amounts

The legacy core fixture uses exactly `1000000000000000000` raw units. The existing core validator requires this total. These are historical test units. They do not represent the approved native total of `1000000000000000` udgt. Native DGT and DRT now use six decimals under the [token unit contract](token-unit-contract.md).

| Bucket | Share | Raw fixture units | Synthetic recipient |
|---|---:|---:|---|
| Ecosystem growth | 30% | 300000000000000000 | fixture-ecosystem |
| Team and advisors | 20% | 200000000000000000 | fixture-team |
| Public sale | 15% | 150000000000000000 | fixture-public |
| Private sale | 15% | 150000000000000000 | fixture-private |
| Reserve | 20% | 200000000000000000 | fixture-reserve |

Only the five bucket shares match the approved allocation reference. The recipient names, dates, staking inputs, and other parameters are synthetic. The legacy four-field DRT split is a test input. It does not replace the approved three-pool DRT split.

## Time and accounting checks

The fixture starts at Unix timestamp `1700000000`. The team schedule has a 10-second cliff and ends 30 seconds after the start. The ecosystem schedule has no cliff and ends 30 seconds after the start. Other allocations have no fixture lock. These short schedules test arithmetic. They do not define production vesting terms.

Tests use explicit timestamps. The expected team amounts cover the start, both cliff boundaries, intermediate release, and both final-release boundaries. Every result must satisfy `vested + locked = allocation`.

The transfer helper must accept the exact cumulative vested amount and reject one additional raw unit. This check includes unknown recipients. A zero-amount helper query can succeed with no allocation.

The fixture validator uses the reserve recipient and bonds 1,000 raw units. Initial allocation accounts must sum to the fixture total. Runtime initialization must debit that stake once. Liquid balances plus bonded stake must still equal the fixture total. Imported JSON must produce the same balances, stake total, and vesting result.

## Qualification limits

The three original failures compared a historical `10^18` input total with `10^27` expected amounts. The independent fixture removes that mismatch and tests allocation and runtime accounting together. It does not change the legacy production template or its fixed-total validator.

`GenesisBlockCreator::can_transfer_at` checks a cumulative vested allocation. It does not check prior spending. The runtime transfer method does not use this helper to enforce a vesting lock. These tests do not establish runtime vesting enforcement, custody controls, or transaction authorization.

JSON import checks reconstruction from configuration. It does not test a database restart or crash recovery. The fixture public key is a placeholder. These tests do not establish validator key validity or consensus readiness.

The native genesis importer has a separate test with six-decimal conversions, the approved five shares, and synthetic recipients with `"vesting": {"kind": "unlocked"}`. It checks `10^15` udgt after import and database reopen. It also rejects historical `10^18` and `10^27` totals, whole-token aliases, and supplied vesting schedules that it cannot enforce. Missing vesting remains development compatibility only. These checks do not convert historical state or approve production recipients.

Full-genesis issuance policy, recipients, custody, vesting schedules, validator records, canonical genesis generation, and runtime lock enforcement remain open. Production schedules must be explicit inputs. Historical schedules must not supply missing production terms. Local fixture success does not close those requirements.
