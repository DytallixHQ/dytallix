# Genesis funding and restart contract

This contract covers monetary initialization. It does not approve a production genesis.
Token precision, allocation policy, and the approved adaptive emission integration remain separate requirements.

## Selected node

`genesis::initialize` accepts exclusive mutable storage, a chain ID, and optional raw JSON bytes.
The executable calls this function before it creates cached account state or staking state.
It reads the existing `genesis.json` path once. The monetary parser reads those bytes directly.
The separate metadata parser does not supply monetary values.

Amounts must be decimal digit strings within u128. Fractions, signs, exponents, numeric JSON
values, and overflow return errors. Numeric JSON values were previously skipped by this
node; they were not credited. The core genesis format has a different amount codec.

The selected format requires a matching chain ID and an accounts array. It permits only
`udgt` and `udrt` balances. Account addresses must be nonempty and contain no whitespace
or control characters. This is an identity check, not full address or key validation.
Duplicate account addresses, denomination keys, and delegation addresses return errors.
The typed decoder also rejects repeated known fields. Unknown metadata remains permitted.

Both `staking.delegations` and `staking.user_delegation` contribute to the same plan.
Either representation can appear alone. A delegation must name an allocated account.
The plan subtracts its stake from that account's DGT allocation. It rejects insufficient
funding. Zero balances and zero stake remain permitted. Duplicate zero entries still fail.

Let A_i be account i's DGT allocation and S_i its genesis stake, with zero for an account
without stake. Checked subtraction requires 0 <= S_i <= A_i. The remaining balance is
L_i = A_i - S_i. Therefore L_i + S_i = A_i for each account. Summation gives:

    sum(L_i) + sum(S_i) = sum(A_i).

Account and delegation uniqueness prevent counting one allocation twice. Checked sums
reject overflow. The existing cap bounds sum(A_i) by 1000000000000000 raw udgt units.
The initializer stores that gross sum in the existing cumulative DGT mint counter.
The runtime cap check therefore includes genesis stake as already issued DGT.
This proof concerns accounting at initialization. It does not prove later settlement.

The initial DRT allocation sum is checked separately. The new `supply:drt_genesis` key
records that sum. It is not a complete DRT supply counter and does not impose a DRT cap.
No amount is rescaled. Neither the core template nor the development genesis files are
changed to select a mainnet allocation or token precision.

## Persistent state and restart

After validation and serialization, one synchronous RocksDB WriteBatch writes:

- Account balance maps at the existing account keys.
- Delegator records with funded stake and zero reward cursors and rewards.
- The existing total stake and DGT minted counters.
- The initial DRT allocation counter.
- Chain ID and the versioned monetary initialization marker.

Atomicity and durability rely on RocksDB's synchronous batch contract. No device-failure
or power-loss test was performed. Opening storage can create database files even when
validation rejects the genesis. A rejected input writes no key-value state.

The marker contains version 1 and a domain-separated SHA-256 digest. The digest covers
the configured chain ID, source presence, and exact genesis bytes. It is an import marker,
not a consensus genesis hash or a state root. The existing genesis-hash RPC is unchanged.
Height and best hash retain the storage defaults of zero and `genesis` until block writes.

A matching marker and persisted chain ID permit reopening without monetary writes.
Later balances, mint counters, stake records, and reward data remain unchanged by the
initializer. A changed, reformatted, or removed source fails the marker check. Preserve
original source bytes. A nonempty database without the marker requires an explicit migration.
This implementation does not guess a previous genesis or rewrite an existing database.

A missing file still permits an empty development store. Such a store receives a distinct
marker. It cannot later import an unrelated genesis over its initialized state.
The executable still reloads configured staking reward rates and other runtime parameters.
This batch does not define governed parameter persistence. The restart preservation claim
applies to monetary initialization, not every later operation in executable startup.

## Core transient runtime

`RuntimeState::from_genesis` validates its input and starts with empty balances. It does
not inherit the default development seed. The constructor sets total DGT supply to the
checked gross allocation sum. Each validator's self-stake must debit a matching allocation.
Registration and delegation errors propagate to the caller. Failure publishes no state.
The final liquid-plus-stake check uses the same conservation identity as above.

The core staking helper uses ordinary additions. Within this constructor, each validator
is unique, the state starts empty, and each positive stake is bounded by its own allocation.
Every partial stake sum is at most the checked gross allocation total. These conditions
exclude overflow in those helper additions on this constructor path only.

The core format has no initial DRT allocation map. The constructor rejects a nonzero DRT
initial supply instead of inventing recipients or dropping that supply. The historical
validator template names addresses without matching allocations; this constructor now
rejects that template. No allocation was moved to make the template pass.

The default runtime development constructor remains unchanged. Core account creation and
core storage import still use different representations. This batch does not unify them.
Vesting metadata still does not constrain spendable balances or stake. Validator activation
policy, commission checks, key validation, and consensus qualification remain separate work.

## Verification boundary

Sixteen new tests cover the selected initializer and core constructor. They check supply
conservation, funded stake, duplicate inputs, malformed amounts, overflow, the unchanged
mint cap, historical template rejection, and restart persistence. Both development genesis
files are accepted. An executable check tests rejection, initialization, restart, and changed
source bytes in private temporary state. Every executable run stops at or before the missing
validator-key gate. It does not sign transactions or start network services.

Read-only balance queries use the existing account snapshot helper. That helper reads
storage when the account cache is empty. This preserves funded balance queries immediately
after initialization and reopening. The reviewer identified the missing fallback. Its
regression failed before correction and passed afterward.

Runtime DRT supply reporting still uses the emission configuration and its emitted counter.
It does not consume the new initial allocation counter. That discrepancy predates this
batch. Correct initial allocation accounting does not close runtime DRT supply accounting.

These checks establish the tested local behavior. They do not establish economic stability,
cryptographic assurance, distributed consensus, or mainnet readiness.
