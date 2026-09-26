# Transaction settlement contract

This contract covers the selected node's active `execute_transaction` path. It does not
establish block atomicity, complete supply accounting, or mainnet readiness.

## State transition

Execution holds the shared storage execution lock from its first durable read through its
commit. The same Storage handle serializes execution across cloned State handles. It reads
account balances and nonces from storage through fallible decoders. It does not convert read
or decoding errors into zero balances. A local map provides subsequent reads within the
transaction. No account-cache or database mutation occurs during planning.

Invalid nonce, exhausted nonce, a message sender that differs from the transaction sender,
an unrepresentable legacy gas limit, or insufficient upfront fee rejects before charging.
Those rejections change no execution state. Signature verification remains an upstream
admission requirement. The sender comparison does not replace signature verification.

The current fee rule remains gas limit multiplied by gas price, charged in DRT base units (`udrt`). Legacy transactions use their fee as a u64 gas limit with price 1. The conversion
now rejects overflow instead of truncating. Multiplication of two u64 values fits u128.
The receipt's legacy `fee` field remains the input transaction fee. The actual charge is
recoverable from the receipt's gas limit and gas price. A future receipt schema must make
that distinction explicit before a production fee contract is approved.

After fee acceptance, execution stages the fee debit and checked nonce increment. It also
stages an addition to `execution:v1:withheld_udrt`. This counter records fees withheld by
this execution version. It does not reconstruct historical fees, distribute fees, or burn
tokens. An overflowing or undecodable counter causes a storage/settlement error without
committing a partial charge. No token precision or fee allocation was selected here.

The fee-and-nonce state forms a checkpoint. Successful messages extend that checkpoint.
A normal message failure discards every message effect and commits only the checkpoint
and failed receipt. This preserves the current accepted-failure fee rule. A storage or
decoding failure discards the entire plan and returns an outer error instead of a normal
failed receipt. The caller must stop execution and investigate storage or block recovery.

## Conservation and arithmetic

Each transfer uses one account entry per address. For distinct addresses, checked subtraction
requires sender balance A >= transfer x. Checked addition requires recipient B+x <= u128::MAX.
The resulting balances are A-x and B+x, whose sum is A+B. A self-transfer checks available
funds and preserves the single account balance. It cannot overwrite a debit with an independent
credit based on the old balance. Repeated messages read prior staged changes.

For DRT, fee charge f reduces the payer by f and increases the withheld-fee counter by f.
Therefore the sum of affected DRT balances plus that counter is unchanged by the fee step.
Transfers and inactivity-switch claims conserve each denomination. Under these admitted
operations, the combined transition conserves balances plus withheld fees. This identity
covers execution version 1 only. Other mint, burn, reward, and governance paths are not part
of this proof and do not yet share this accounting boundary.

Nonce increment requires nonce < u64::MAX. Transfer subtraction, recipient addition, fee
accumulation, inactivity periods, and inactivity deadlines use checked arithmetic. Invalid
amounts do not wrap or saturate into a different accepted amount.

## Inactivity-switch state

Dead-man-switch registration and ping stage records at the existing `dms:config:{owner}` keys.
Their reads see earlier staged records in the same transaction. Registration requires a
positive period and a distinct beneficiary. Periods must fit u64. Registration and ping
must produce a representable deadline. Ping cannot move the last-active height backward.

Claims use checked deadlines and transfer the owner's balances to the named beneficiary.
A later message failure discards earlier registrations, pings, and balance transfers.
An invalid stored encoding is a fatal execution error, not a missing switch. The direct
legacy switch module remains available; this batch changes its active execution caller.

## Commit and retry

One synchronous RocksDB WriteBatch contains account balance maps, account nonces, changed
switch records, the withheld-fee counter, the ordinary receipt, and a versioned settlement
record. The state cache changes only after the batch succeeds. The test writer can return
an error before the write; the injected-error test verifies unchanged storage and cache.
Durability and atomicity depend on RocksDB's contract. Device failure, ambiguous I/O failure,
and power-loss behavior were not tested. A write error stops production; it does not prove
that an external device failed before every durable write. Recovery must inspect storage.

The settlement record uses a domain-separated SHA-256 digest of the serialized transaction.
The record binds that input to the requested height and index. An identical retry at the
same position returns the recorded receipt without another fee, nonce, or diagnostic update.
A changed input, position, unsupported record version, or invalid record encoding fails.
The caller must not reinterpret the error as a fresh transaction. This record is not a
consensus block commitment or a complete recovery protocol.

Execution clears the local account cache on an identical retry so later queries use durable
state. Legacy RPC receipt writers and block writers can still write outside the execution
lock. The dedicated settlement record remains distinct from their ordinary receipt key.
Those external writes require integration in the later block settlement work.

## Fee burn diagnostics

The existing fee-burn engine tracks diagnostic totals in memory. It does not actually burn
tokens or enforce DRT supply. Execution prepares a clone of that engine and publishes it
only after a successful transaction commit. A matching retry does not count it twice.
Diagnostic failure remains nonfatal and is logged. The default token, rate, threshold, and
success-only behavior remain unchanged. Wall-clock diagnostic event times remain outside
the durable settlement record.

The diagnostic calculation now computes floor(f*r/10000) as
floor(f/10000)*r + floor((f mod 10000)*r/10000), for 0 <= r <= 10000.
The first term is at most f and the remainder product is less than 10000 squared. Their
sum is at most f. The calculation therefore supports u128 fees without an overflowing
intermediate product. Accumulated diagnostic totals use checked addition.

## Caller and remaining requirements

`execute_transaction` now returns Result<ExecutionResult>. An outer error reports an
execution-storage or integrity failure. A returned failed ExecutionResult remains a normal
transaction failure. Repository callers explicitly handle this API change. Existing test
assertions remain unchanged, including the established out-of-gas error format.

The actual producer enters a terminal failed state and exits its production task on an outer error. It does not
remove that transaction from the mempool or write a fabricated success. The rest of the
node can remain running. Status and health responses report the failed production state.
Pause and resume return HTTP 503 after failure. Atomic state transitions prevent a concurrent
resume from clearing failure. Normal operator pause/resume behavior remains available before
failure. Production requires investigation, explicit block recovery, and restart. The state
is process-local; it is not a durable recovery controller or a mainnet readiness gate.

The producer still advances emission before transaction settlement, writes blocks separately,
filters charged failed transactions from block contents, and suppresses block-write errors.
A crash can therefore leave committed transactions without a committed block. Complete
block staging must include emission, all transaction outcomes, receipts, block data, height,
and the resulting state commitment in one transaction before mainnet qualification.

Legacy direct account setters, State::apply_transfer, faucets, emission claims, staking claims,
governance writes, and core execution do not yet share this boundary. Mempool fee reservation
also differs from current execution charging. Runtime DRT supply integration, vesting, approved
adaptive allocation/timebase, consensus, migration, independent assurance, and launch approval
remain open. These limits must not be hidden by passing transaction tests.
