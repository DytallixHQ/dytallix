# Selected-node admission contract

This contract aligns queue admission with the current selected-node execution rules.
It does not approve the mainnet fee model or adaptive issuance. The user approved
six decimal places for both DGT and DRT; see `token-unit-contract.md`.

## Fee and transfer units

`transaction_cost::effective_gas` is the common selector for admission and execution.
When both gas fields are positive, the upfront fee equals gas limit times gas price.
Otherwise, the legacy fee must fit the gas-limit u64 domain and uses unit price.
The product of two u64 values fits u128. The node reserves this fee once in UDRT.
It does not add the top-level legacy fee to the explicit gas product.

DGT transfer funding cannot pay a DRT fee. Transfer and fee requirements remain separate
except when the transfer itself uses DRT. Required balances use checked arithmetic.
Reservation overflow returns an error; it does not saturate to an affordable value.

Public DGT/DRT aliases denote whole tokens under the existing development input scale of
10^6 base units. Public UDGT/UDRT aliases denote base units. Conversion normalizes both
case and amount before queue validation and storage. Multiplication overflow fails.
Existing balances are not rescaled. This selected-node alias conversion does not
by itself qualify production genesis balances or migrations.
Stored transfer messages must use canonical `udgt` or `udrt` names. Signature validation
of the original public envelope still occurs before conversion.

A transaction reserves the peak sender balance required by its ordered messages. Start
with the upfront UDRT fee as spent. For a send of amount A, calculate spent+A with checked
addition and increase the required balance to that peak. A distinct-recipient send also
increases spent. A self-transfer requires the amount to be available but does not increase
spent. Every message must identify the enclosing sender.

Requirements from separate queued transactions add conservatively. Admission does not
assume later incoming credits or rewards. This policy can reject a transaction that would
succeed after an unconfirmed credit. It protects existing reservations without changing
consensus execution or allocating fees. The current pending-transaction display total also
has a checked u128 bound; mixed-token display representation remains a separate API concern.

## Queue invariants

The queue keeps one transaction per sender and nonce. It reserves funds for both ready
and deferred transactions. A duplicate cannot overwrite its predecessor. Ready means that
the nonce belongs to a contiguous sequence beginning at the observed committed nonce.

Admission first checks signatures, policy, identity, size, gas, and available balances.
When observed committed nonces are unchanged, capacity is available, and insertion cannot
promote a deferred successor, admission updates only the affected sender and indexes.
It calculates checked byte counts, nonce ranges, ready counts, and reservations before
changing the live queue. No error-returning operation follows the first live update.

Promotion, capacity eviction, and changed committed nonces use a candidate queue. Indexes,
byte counts, nonce readiness, and reservations must all complete before the candidate
replaces the live queue. The public submission path also persists Pending records before
publishing its candidate.

Reconciliation reads the current State after block settlement. Removed hashes do not
imply nonce consumption. Charged failures advance the committed nonce. Pre-fee rejections
do not. Eviction or removal of a predecessor leaves successors deferred. An exhausted
nonce cannot enter the queue.

Selection compares fees only among each sender's next ready transaction. After selecting
one transaction, it exposes that sender's successor. Thus a higher-priced successor cannot
pass its predecessor. Priority uses the full unsigned u64 gas-price domain. Equal-priority
transactions retain nonce and hash tie breakers. Snapshot order is deterministic.

`len` reports ready entries. `total_count` reports ready plus deferred entries. Capacity
checks use the total count and bytes. `drop_hashes` only removes entries; it cannot infer
chain progress. Production uses `reconcile` with updated State after settlement and before
selection. Callers that simulate inclusion must supply the resulting State explicitly.

The rebuilt reservation for token D and sender S is the checked sum of the retained
transactions' requirements for D. Before admission, this sum plus the candidate requirement
must not exceed the available balance. Eviction can only reduce that sum. Rebuilding from
retained entries avoids lossy inverse calculations after overflow or promotion.

## Compatibility and verification limits

Existing queue fixtures now fund DRT fees separately from DGT transfers. Tests that used
ready counts as capacity counts now check total retained entries and the expected deferred
entries. Ordering controls require sender nonce order before fee priority. Existing
rejection labels remain intact, including insufficient funds and duplicate ready nonces.

This batch does not add queue persistence, distributed admission, complete signed-envelope
replay, signer-to-address assurance, full supply accounting, vesting, or a mainnet fee policy.
Those requirements need separate evidence. Public aliases now produce the base-unit amounts
that the former public balance validator required; old client expectations must be checked
before any release or migration.

Ordinary admission avoids a whole-queue clone and rebuild. It still checks each retained
sender's committed nonce because State has no nonce revision counter. Ready duplicate checks
use the contiguous sender nonce interval. Deferred duplicate checks use the sender index.
Promotion, eviction, removal, and reconciliation retain queue rebuilds and their increasing
cost. The public submission path retains its persistence candidate clone.

Local measurements must separate transaction preparation, signature verification, queue
updates, and selection. Passing queue tests does not qualify public submission throughput
or mainnet capacity. Existing performance limits remain unchanged.
