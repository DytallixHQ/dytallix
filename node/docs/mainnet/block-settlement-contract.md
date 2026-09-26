# Development block settlement contract, version 1

This contract applies to the selected `dytallix-fast-node` producer. It defines a local
commit boundary. It does not define distributed consensus or approve a mainnet protocol.

## Activation and compatibility

Set `DYT_BLOCK_PROFILE=development` explicitly. Missing or other profile values stop
startup. Mainnet adaptive issuance remains inactive. The approved model is the corrected
whitepaper adaptive model. This batch does not replace that decision.

The development profile retains the existing static, phased, or percentage emission
calculation for supported inputs. Its default split is 60/25/10/5. Its percentage schedule
retains the existing bootstrap, floor, and assumed blocks per year. These are development
semantics, not approved mainnet allocation or timing rules. No token precision changes.

An explicit emission configuration must load and decode successfully. Startup no longer
substitutes the default after a file or decoding error. Configuration validation rejects
invalid shares, phase intervals, overlapping phases, and rates above 100 percent. The
persisted policy digest binds the emission configuration, staking flag, and gas schedule.
A changed policy requires a reviewed migration.

Governance activation and direct funding endpoints stop startup. Direct emission claims
return HTTP 400. Direct staking delegate, undelegate, and claim routes return HTTP 501,
including when staking accounting is enabled. Staking reads remain available. The node
can stage staking emission against genesis-funded stake. Signed claim and stake mutation
transactions are future work. The legacy autonomous staking reward-rate setter no longer
runs at startup. That rate does not control this profile's external emission accounting.

Legacy version-zero JSON blocks remain readable. The new producer does not extend a
legacy history. Unmarked execution, block, or emission records require migration. The
standalone transaction executor and legacy block writer reject databases with a block
settlement head. Legacy library helpers remain available for isolated tests; they do not
provide this block contract.

## Planning and commit order

1. Snapshot the transaction queue and pending asset prefix.
2. Lock emission, staking, account state, and fee diagnostics in that order.
3. Acquire the storage execution lock. Verify committed history and current state.
4. Check the policy and requested height. Handle a matching last-block retry without
   executing transactions again. Reject a changed retry or an invalid height.
5. Stage ordered transactions against one account, nonce, fee, and switch map.
6. Stage one emission transition and the optional staking transition for the same height.
7. Calculate the transaction, receipt, request, and selected-state digests. Build the block.
8. Write account state, fees, switches, lifecycle state, receipts, block, indexes, policy,
   and head in one RocksDB WriteBatch with synchronous write options.
9. Publish caches and fee diagnostics after the write succeeds.
10. Remove considered queue entries. Remove only the committed asset prefix. Publish block
    metrics and notifications after commit. Stop production on a settlement or queue error.

Transaction timing measures local staging. Block timing includes settlement. Timings stay
outside deterministic commitments. A matching retry does not add timing observations.

The transaction queue publishes admission only after transaction and Pending receipt
storage succeeds. Admission uses a cloned queue candidate. A failed admission write does
not publish insertion or capacity eviction. The storage execution lock serializes Pending
writes with block settlement. A late Pending write cannot replace a terminal receipt.

A pre-fee rejection produces a terminal Failed receipt with no block height or index. It
has no fee debit or nonce increment. It is not included in the block. Its receipt joins
the block batch when a block exists. A tick with only rejections writes those terminal
receipts in one synchronous batch; it does not advance emission or block height.

An accepted transaction pays its upfront fee and advances its nonce. A failed message
restores the accepted fee-and-nonce checkpoint. It discards every message effect from that
transaction. The block includes the charged failure and its receipt. Subsequent accepted
transactions use contiguous indexes and all prior staged effects.

An empty tick with empty blocks disabled writes nothing. Pending assets can cause a block
even without transactions. Asset acknowledgement removes only the captured prefix. New
arrivals remain queued. Pending asset and transaction queues remain volatile. Persistent
admission records do not yet reconstruct the queue on restart.

## Arithmetic invariants

Let `F` be withheld DRT fees and `f_i` an accepted transaction's upfront fee. Staging uses
`F_next = F + sum(f_i)` with checked addition. Repeated transactions read the staged fee
total. A failed message still contributes its accepted fee. A pre-fee rejection contributes
zero. The fee counter records withholding; it does not allocate or burn tokens.

For each transfer, debit and credit use checked arithmetic. A self-transfer leaves the
balance unchanged. Distinct accounts preserve their combined balance. Message rollback
preserves earlier transactions and the current fee checkpoint. Thus the supported account
transition preserves transferred units, apart from the explicit fee debit into withholding.
This statement does not establish whole-chain DRT supply accounting.

For an emission amount `n`, share `p`, and denominator `d`, calculate the exact floor as:

`floor(n*p/d) = floor(n/d)*p + floor((n mod d)*p/d)`.

The implementation checks each intermediate result. For shares, `d=100` and `0<=p<=100`.
The first three shares receive their floors. The fourth receives the exact remainder.
Since shares sum to 100, allocated amounts cannot exceed `n`, and their sum equals `n`.
The remainder rule preserves the existing development allocation convention.

For staking, let `A` be the new staking allocation plus previously pending emission,
`S` the positive total stake, `Q=10^12` the scale, and `R` the stored remainder. Set
`N=A*Q+R`, `index_next=index+floor(N/S)`, and `R_next=N mod S`.
The implementation checks multiplication and addition. Therefore
`S*(index_next-index)+R_next=A*Q+R` and `0<=R_next<S`.
When `S=0`, retain the allocation as pending emission. An arithmetic limit stops the block
before any write. The index and pool represent related accounting records; this contract
does not authorize two independent claims against them.

## Commitments and recovery

Version-one block hashing uses a domain prefix and deterministic JSON encoding of the
complete header and transaction bodies. The header binds ordered assets, transaction
count, transaction digest, receipt digest, request digest, and selected-state digest.
The request includes height, timestamp, ordered considered transactions, assets, the
empty-block choice, and policy. The receipt digest includes charged failures.

The selected-state digest includes account, nonce, switch, emission, staking, supply,
genesis, chain-ID, withheld-fee, and policy records. Keys are sorted. New emission events
encode pool entries in sorted order. The existing binary EmissionEvent decoder remains
compatible. Fee diagnostics, pending admission, contracts, bridges, oracles, and unrelated
module records are outside this state digest. It is not an approved consensus state root.

Recovery verifies each local block, parent, index, transaction member, settlement record,
ordinary receipt, receipt digest, and head metadata. It checks current selected state
against the committed digest and rejects orphan settlement or block records. It checks
that emission height equals block height. Legacy partial execution is not reconstructed
by guessing which transactions belong in a block.

A matching last-block retry returns committed results and refreshes account, emission,
and staking caches. It performs no second database write, fee debit, or lifecycle advance.
A different request at that height fails. Older retries require explicit history handling.

A failure before database write leaves durable state and caches unchanged. A lost commit
acknowledgement is different: the database can contain the complete block while caches
remain stale. Production stops. Verification and a matching retry recover that complete
commit once. Tests inject both conditions. They do not simulate physical power loss or
prove storage-device durability.

Recovery digests detect inconsistent local records. They do not authenticate a database
against a party that can rewrite records and recompute every digest. External checkpoints,
validator signatures, consensus finality, and operator recovery approval remain necessary.

## Qualification limits

The current verifier scans history and selected state before each block. It is a local
qualification baseline with growing cost. It is not a mainnet performance design. A future
version needs an incremental authenticated state structure and measured recovery bounds.

Complete adaptive journal integration, monetary settlement, supply accounting, vesting,
fee reservation, fees and burns, signed module transitions, consensus, synchronization,
cryptographic assurance, operator qualification, migration, and release testing remain open.
Neither these tests nor this contract establish mainnet readiness.
