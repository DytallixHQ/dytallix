# Genesis arithmetic and allocation import contract

## Vesting domain and exact result

Let A be the allocation amount, s its start timestamp, c its cliff duration, and d
its total duration. Amounts use unsigned 128-bit integers. Times use unsigned 64-bit
integers. Valid inputs satisfy 0 <= c <= d and s+d <= u64::MAX. A may be zero or any
value through u128::MAX. A vesting schedule attached to an allocation must have the
same total amount as that allocation.

For an agreed timestamp t, the cumulative vested amount V(t) is:

- Zero when t < s+c.
- A when t >= s+d.
- floor(A*(t-s-c)/(d-c)) otherwise.

If c=d, the intermediate interval is empty. The complete amount unlocks at s+c.
This includes c=d=0. The implementation preserves that existing behavior.

In the intermediate interval, let e=t-s-c and p=d-c. Then 0 <= e < p and
1 <= p <= u64::MAX. Write A=q*p+r with 0 <= r < p. The implementation computes
q*e + floor(r*e/p). This equals floor(A*e/p) exactly.

The first product fits u128 because q*e <= q*p <= A. The second product fits because
r*e < p^2 <= (2^64-1)^2 < 2^128. The final sum is at most A, so it also fits. Checked
operations remain in the implementation to return errors if an invariant changes.
Timestamp validation occurs before evaluation. Branch order prevents subtraction
underflow and division by zero.

V(t) is nondecreasing because e increases with t and floor preserves order. It remains
between zero and A. The locked amount is A-V(t), so locked plus vested equals A
exactly. In the linear interval, the difference from the rational amount is less than
one smallest unit. This proof does not depend on the selected token decimal scale.

These are cumulative allocation queries. They do not deduct prior spending or claims.
`can_transfer_at` is a compatibility query for that cumulative amount. It is not
transaction authorization. Production vesting needs account state that records spent
or claimed amounts and uses an agreed consensus timestamp.

## Checked APIs and allocation identity

Vesting queries, allocation-total queries, account creation, and runtime construction
now return `Result`. Invalid schedules, timestamp overflow, duplicate allocation
addresses, empty addresses, mismatched vesting totals, and aggregate overflow return
errors. Callers must handle those errors. No error becomes a fabricated zero amount.
A missing allocation address still has zero vested and locked amount.

`get_vested_amount_at` and `get_locked_amount_at` accept an explicit timestamp.
The existing current-time methods remain diagnostic wrappers. They reject a clock
before the Unix epoch. Core genesis validation rejects a pre-epoch genesis timestamp.
The block-validation entry point and direct account creation validate their supplied
configuration. Runtime construction also performs configuration validation first.

The raw core-template total remains 1000000000000000000. The constructor's historical
`mainnet` name does not approve that template for launch. Three integration tests still
expect a total or allocations at a different scale. Their amount assertions remain
unchanged. Their vesting checks now use explicit time instead of the current date.

## Amount encoding and compatibility

Genesis monetary fields serialize as decimal strings in JSON. This includes allocation
amounts, vesting totals, initial DRT supply, proposal threshold, minimum validator stake,
and per-block emission. Validator stake already used string encoding.

The new adapter accepts exact JSON integer literals through u128::MAX and decimal
strings through u128::MAX. It rejects negative amounts, fractions, exponents, overflow,
and strings containing non-digit characters. It reads the raw JSON value and then uses
an integer parser. It never converts an amount through floating point.

The core dependency enables serde_json's `raw_value` feature. Package versions and the
lockfile remain unchanged. This feature exposes raw JSON input without changing number
parsing globally. Binary amount serialization retains the previous u128 representation.
The raw adapter is for JSON and binary Serde formats; other human-readable formats are
not qualified by this batch. Configuration decoding alone does not validate economics.

## Atomic storage initialization

The core storage importer accepts a required `dgt_allocations` array or its `allocations`
alias. Other document fields remain outside this allocation importer's validation scope.
It does not establish a complete genesis configuration commitment or validate a network
configuration embedded in the document.

Every allocation must have a nonempty unique address, a valid schedule when present,
an exact amount, and a representable aggregate total. This importer retains its existing
`dyt1` address-family requirement. Unsupported addresses now return errors instead of
being skipped. The prefix check is not full address or public-key validation.

The importer parses and validates every entry and encodes every account before writing.
One synchronous RocksDB WriteBatch contains all accounts, chain ID, height zero, best
hash, and the initialization marker. Parse or validation failure leaves the key-value
database empty. Opening RocksDB can still create its directory and database files.
The synchronous write relies on RocksDB's atomic-write and durability contract. This
batch does not include device-failure or power-loss injection.

The marker contains a domain-separated SHA3-256 digest of the exact supplied file bytes.
An empty development import has a distinct digest. Once the marker exists, reopening
checks the digest and chain ID and does not reapply allocations, including at height
zero. A changed, removed, or reformatted genesis file therefore prevents reopening when
that file was the original import source. The genesis file is a retained startup input.

A nonempty database without the marker requires an explicit migration. The importer
does not infer freshness from height zero, guess a previous genesis, or overwrite the
database. No migration is supplied in this batch. Keep the database and original genesis
file intact for migration review. `open(..., None)` still permits an empty development
store. The convenience `new()` preserves its no-file development default; an explicitly
selected missing file returns an error.

Storage imports credit the allocation balance. They do not enforce vesting, fund stake,
or settle tokens. Those runtime requirements remain open.

## Remaining mainnet requirements

Token precision remains a requested decision. The selected fast-node convention, core
raw amounts, and older examples disagree. No balance rescaling occurred in this batch.
Emission allocation, timebase, fee rules, burn settlement, and launch-module decisions
also remain open. The corrected adaptive emission model remains the selected target.

Core runtime state still has separate seed-balance, supply-accounting, and stake-funding
problems. Validation at construction does not resolve them. Full genesis commitment,
validator key validation, distributed consensus, vesting enforcement, atomic monetary
settlement, migration, and independent qualification remain required.
