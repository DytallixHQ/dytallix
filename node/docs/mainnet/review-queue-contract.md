# Review-queue state contract

The core review queue stores manual review records in memory. It does not authorize
an officer, execute a transaction, or establish distributed consensus.

## State ownership

One read/write lock protects canonical records, pending IDs, and the active hash
index. Admission checks duplicates and capacity while it holds this lock. A state
update does not await another lock, statistics calculation, or notification service.

Pending IDs refer to canonical records. The queue no longer keeps separate copies
that can retain an obsolete status. Pending records and in-review records both count
against `max_queue_size`. Completed records do not count against active capacity.

The queue preserves priority order. Equal-priority entries keep their insertion order.
`get_next_for_review` now selects without removing an entry. It has no officer argument
and cannot establish a review claim. `start_review` claims a selected ID once.
`claim_next_for_review` selects and claims the next entry in one locked operation.
Callers that previously treated selection as a claim must use the claim method.

## Allowed transitions

| Current state | Allowed next states |
|---|---|
| Pending | InReview, Approved, Rejected, Expired |
| InReview | Approved, Rejected, Expired |
| Approved, Rejected, Expired | None |

Approval and rejection still permit a decision directly from Pending. Officer
permissions remain the caller's responsibility. The queue does not infer officer
identity or require the same officer to claim and decide a record.

A claim removes the ID from the pending list but retains its active hash. A final
decision removes the pending ID and active hash together. The canonical record
retains the decision. A repeated or conflicting final decision returns an error.
It cannot change the record or remove a later record's hash index entry.

Hash deduplication covers active review records. Re-enqueue after a final decision
remains allowed. This is not a chain-level replay or transaction-nonce policy.
Bulk operations retain per-record success and failure handling. They are not atomic
as a group.

## Expiry, statistics, and cancellation

Expiry checks eligibility and records the final decision under the same lock used
by approval. It cannot apply an earlier eligibility result over a later approval.
Out-of-range expiry durations return an error before any queue update.

Statistics come from one canonical snapshot. Each read starts its counters at zero.
Daily approval and rejection counts use the UTC date. Elapsed review time retains
the existing definition: time from queue admission to a final decision, in minutes.
Pending age reflects the current time even when the queue has not changed.

Cancellation while waiting for the state lock does not insert a record. Once a
mutation obtains the lock, it has no asynchronous suspension point before commit.
Notifications run after the lock is released. Cancellation after a state update can
leave that update committed without a notification or caller acknowledgement.
Callers must read current state before deciding whether to repeat an operation.

## Remaining production requirements

The queue is not persistent. Historical records remain in memory without a retention
bound. Statistics read that history, so their cost grows with retained records.
A single lock also serializes mutations. Production load and retention limits remain
unqualified.

Durable review records, notification delivery, access control, audit integration,
retention policy, and restart behavior require separate implementation and acceptance
criteria. This batch does not connect manual review state to mainnet finality.
