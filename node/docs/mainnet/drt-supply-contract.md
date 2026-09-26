# DRT supply accounting contract, version 1

This contract covers DRT in the selected development block lifecycle. The approved
mainnet emission model remains the corrected whitepaper adaptive model. This contract
does not select its allocation, parameters, timebase, fees, or burn rules.

## Accounting identity

All amounts are raw `udrt` integers. Define:

- G: DRT allocated by the monetary genesis initializer.
- E: cumulative DRT emitted into the four current emission pools.
- L: sum of DRT in stored account balance maps.
- F: cumulative withheld DRT fees.
- P: sum of DRT held in the four emission pools.

The selected lifecycle has no settled burn transition. It must satisfy:

    Total = G + E = L + F + P.

Withheld fees remain part of supply. A diagnostic burn event does not change this identity.
The general mainnet identity will include recognized burns: Total = G + E - B. Implement B
only with an approved atomic burn transition. This implementation rejects unknown DRT
supply records. It cannot silently apply a future supply format as version 1.

The staking reward index, pending staking emission, accrued reward claims, and rounding
residuals describe claims on a pool. They are not additional custody balances. Adding them
to P would count the same issued DRT twice. This contract checks custody conservation.
It does not prove that every reward claim has sufficient backing or that the reward index
is correct for future stake changes and payouts.

## Conservation reasoning

The proof applies to the selected staged operations, with checked nonnegative integers.
It assumes the genesis source and stored records pass validation.

At initialization, E, F, and P are zero. The initializer sums the allocated DRT balances
into G. Thus G = L. DGT stake funding does not change DRT.

An accepted fee of f changes L to L-f and F to F+f. Their sum does not change.
A transfer of a changes two liquid balances by -a and +a. Their sum does not change.
A self-transfer does not change a balance. The supported inheritance claim uses these
same transfers for the owner's balances. A failure after fee acceptance restores message
effects but retains the fee. A rejection before fee acceptance changes no monetary state.
Thus the transaction stage preserves L+F+P.

For an emission e, the planner increases E by e. It allocates floor(e*p/100) to each of
the first three pools. It allocates e minus their sum to the fourth pool. The nonnegative
shares sum to 100, so their first three floors cannot exceed e. Therefore the total pool
increase is exactly e. Staking index updates do not remove pool custody. Both sides of
the identity increase by e.

Induction over committed blocks preserves the identity. Every sum and difference must fit
u128. A failed check returns an error before the batch write and cache publication.
Atomicity depends on the existing synchronous RocksDB batch contract. The tests do not
simulate device power loss.

## Shared validation and reads

`supply::validate` reads a RocksDB snapshot. It overlays proposed writes before computing
totals. An overlay replaces a stored value; it does not add a second copy. The function
writes no state. It checks exact amount encodings, canonical account maps, the genesis
marker format, known pool names, required counters, and every sum.

Block recovery validates current custody before it validates the existing block journal.
Block planning validates proposed custody after it combines transaction and emission writes.
The block state digest already includes account balances, emission records, supply counters,
and withheld fees. No additional supply cache or mutable total is introduced.

`supply::inspect` acquires the storage execution lock. It verifies recovery and then returns
one current accounting view. It requires a monetary genesis marker. Unmarked legacy
emission history does not qualify as committed supply. The lock coordinates selected block
writers. Arbitrary external database writes and legacy library mutation helpers remain
outside this contract.

`GET /api/supply/drt` returns genesis, emitted, burned, total, liquid, withheld fees, pools,
height, denomination, and accounting version. Monetary amounts are decimal strings.
Invalid state returns HTTP 503 without a partial supply estimate. This query performs
history and state validation. Its cost grows with stored history and accounts. It needs
bounded serving and performance qualification before mainnet exposure.

`EmissionEngine::get_supply_info` now returns a Result. It reads verified storage totals,
not configuration or cached emission values. Its legacy `circulating_supply` field still
means cumulative emitted DRT. It does not mean spendable or market circulation. No current
production caller uses the former infallible helper; external library users must adapt.

## Genesis and configuration compatibility

Restart checks the stored G against the original genesis allocation sum. Missing or changed
G fails. Source-byte and chain-ID checks remain in place. The node does not rescale balances.

Automatic development configuration takes its initial DRT supply from funded genesis.
Explicit configuration must declare that same amount. A mismatch fails startup. Percentage
emission therefore uses actual funded genesis plus cumulative emission. The prior automatic
configuration could use zero despite nonzero funded genesis.

Existing block policy digests remain binding. An older history with an incompatible initial
supply setting requires a reviewed migration. The node does not rewrite policy digests,
genesis bytes, balances, historical blocks, or receipts to make that history pass.

## Mainnet requirements still open

Complete DGT custody accounting and vesting enforcement. Define reward liabilities and
atomic payouts. Integrate the calibrated adaptive controller, approved allocation, and
approved burns with monetary block settlement. Add every enabled module's custody records
and state commitments. Qualify queue and query performance, persistent recovery, distributed
consensus, signed transaction records, cryptography, independent security review, operators,
and the frozen release. These local accounting checks do not establish mainnet readiness.
