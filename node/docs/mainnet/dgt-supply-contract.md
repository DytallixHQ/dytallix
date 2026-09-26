# DGT and stake accounting contract, version 1

This contract extends the selected development lifecycle's DRT accounting check.
It does not approve mainnet allocation, vesting, consensus, or stake activation.

## DGT identity

All amounts are raw `udgt` integers. Define:

- I: the stored cumulative DGT issuance counter.
- L: the sum of DGT in account balance maps.
- S: the sum of `stake_amount` in individual delegator records.
- T: the stored total-stake counter.
- C: the existing DGT cap, 1000000000000000 raw units.

Require:

    L + S = I <= C
    S = T

The cap is a ceiling. It is not evidence that every unit has been issued.
The checker counts stake once. It does not add T to S or include DRT reward claims
as DGT. The selected block lifecycle has no active DGT mint, burn, unbonding, or
stake-change transaction. Those future transitions need explicit accounting rules.

Genesis debits stake from the allocated DGT balance and records that same stake.
For each allocation A and funded stake s, liquid A-s plus stake s equals A.
Summation proves L+S=I at initialization. Account and delegation uniqueness prevent
duplicate entries. Checked sums and the existing cap bound the result.

A DGT transfer debits and credits the same amount. A self-transfer changes no balance.
The supported inheritance transfer uses the same staged transfer operation. Message
rollback restores these effects. Fees debit DRT, not DGT. Emission and staking reward
index updates issue DRT, not DGT. Thus selected block transitions preserve L+S and I.
The checker verifies this identity before every block write and during recovery.
It also verifies S=T from individual records, independently of the cached staking total.

The identity establishes custody conservation within this lifecycle. It does not prove
validator eligibility, voting power, reward-liability solvency, address ownership,
vesting authorization, or consensus finality.

## Shared snapshot and record rules

`supply::validate_native` checks both DGT and DRT from one RocksDB snapshot and proposed
write overlay. Overlaid values replace stored values. The function writes no state.
The existing block planner and recovery call the shared validation boundary.
`inspect_native` acquires the execution lock and verifies the local block journal.
The existing DRT inspection function now also requires valid DGT accounting.

DGT issuance and total-stake counters must exist and use exact u128 encoding.
Delegator records must decode and re-encode to the same bytes. Each record needs a
nonempty address without whitespace or control characters and an account record.
This record link does not independently prove historical funding or key ownership.
The global custody identity and funded genesis provide the supported accounting proof.

All liquid, individual-stake, and combined totals use checked arithmetic. Unknown supply
or staking record families fail validation. The allowed staking metadata consists of
total stake, reward index, pending emission, reward residual, and the legacy reward rate.
Amount and reward-rate encodings must match their stored types. Future unbonding custody,
burn counters, or revised record layouts require an accounting migration.

Reward index, pending emission, and reward-rate values used by statistics come from the
same verified snapshot as the supply totals. They do not come from mutable module caches.
Reward metadata does not add custody to either token total.

## Reporting

`GET /api/supply/dgt` returns height, issued DGT, liquid DGT, staked DGT, the cap,
denomination, and accounting version. Monetary amounts are decimal strings. Invalid
state returns HTTP 503 without partial totals.

`GET /api/staking/stats` now uses verified issued DGT as the staking-ratio denominator.
It reports stored funded stake even when the staking feature flag is off. A flag does
not erase custody. The ratio in basis points is floor(10000*S/I). When I=0, conservation
requires S=0 and the reported ratio is zero. Since S<=I<=C, the product fits u128.
The percentage display truncates to two decimal places. It differs from the exact
percentage by less than 0.01 percentage point. It uses no floating-point ratio calculation.

Statistics add issued supply and the cap as separate fields. The `apy` field is now null,
and `apy_status` is `not_qualified`. The legacy reward-rate setting does not establish
annual percentage yield for the selected external-emission lifecycle. Clients must handle
unavailable yield. Invalid accounting returns the existing HTTP 500 internal-error response
for this route. Validator preview fields retain their prior behavior and need separate
consensus qualification.

These reads still verify full local history and state. Their cost grows with the database.
Bounded query handling and a qualified efficient read model remain mainnet requirements.

## Compatibility and remaining work

No balance, counter, cap, precision, or policy digest is rescaled or rewritten.
Previously inconsistent stake totals or unknown custody records require migration.
The source does not infer missing counters or rewrite balances to satisfy the identity.
Two block fixtures now fund their stake through genesis. Their reward-index and failure
assertions remain in place. Direct legacy mutation helpers remain outside this contract.

Vesting still requires a selected timestamp rule and enforcement on every spending path.
Future staking needs signed delegation, reward settlement before stake changes, unbonding,
validator selection, and atomic payouts. The approved adaptive model still needs calibrated
parameters and integration with allocation and monetary settlement. Independent review,
cryptographic assurance, distributed operation, performance, and release qualification
remain open. Local accounting tests do not establish mainnet readiness.
