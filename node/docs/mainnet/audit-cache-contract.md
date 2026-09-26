# Storage sessions, audit staging, and batch contract

## Storage ownership and encoding

`OptimizedStorage::session` borrows one storage instance for the session lifetime.
Cached reads use that instance. Session start and drop clear cached entries. Callers
must prevent writes through other backend handles while a session is active. The
Rust borrow prevents mutation through the borrowed handle; it cannot control an
independent database connection or an interior-mutable backend.

Session writes update existing cached keys even when the cache is full. Removal
invalidates the cached key. Capacity zero disables cache insertion. Capacity limits
entry count, not encoded byte size. Missing backing reads count as reads and misses.
Metric counters saturate instead of wrapping.

The unbound `optimized_read(storage, key)` method always consults storage. Its caller
can change storage or select another instance between calls. Use a session to obtain
cached reads. A counted storage fixture verifies that three session reads cause one
backing read; capacity zero causes three backing reads.

Map operations use `cw_storage_plus::Map::key` and the standard JSON encoding. They
write only the canonical namespaced key. The old code also wrote an unqualified key
with a different encoding. New reads ignore those old unqualified entries. This batch
does not delete existing data. Existing canonical Map entries remain readable through
both the ordinary Map API and the optimizer.

`batch_save` encodes every item before the first write. Encoding failure leaves all
items unwritten. The Storage trait does not provide rollback for a backend failure
or panic during writes. The separate legacy key-compression helpers remain unchanged;
the canonical Map path does not use them.

## Diagnostic access rates

Access frequency uses total accesses divided by elapsed time from the first recorded
observation. The minimum observation window is one second. Backward clock steps do
not decrease the last observation time. `record_access_at` supplies a testable time
input. The ordinary method uses the local system clock.

The existing caching recommendation requires reads greater than three times writes.
Exactly three reads per write do not satisfy it. Recommendations and gas-saving
estimates remain diagnostic heuristics. They are not consensus inputs or proven gas
bounds. Frequency and size summaries use floating-point values.

## Audit acceptance and staging

One lock owns accepted records, pending IDs, and query counters. A successful record
call makes the entry visible to transaction queries and reports immediately. Updating
compliance status also works before staging completion. No separate index can refer
to a record that has not yet become visible.

`max_memory_entries` bounds all retained records, including records in completed
staging batches. Reaching the limit rejects the next record. Disabled recording
returns an error instead of an invented success ID. Callers must handle these errors.

`flush_pending_entries` completes only an in-memory staging batch. It does not make
records durable or release their retained memory. Pending IDs and batch counters
change together. Statistics count accepted records, not only completed batches.
Daily counts use the UTC date of each record. The configured flush interval does not
start a background timer.

Queries copy one current snapshot. Filters include sender or recipient addresses and
amount bounds. Summaries cover all matches before pagination. Priority, transaction
type, manual-review, and flagged counts use the matching records. Sorting uses timestamp
and audit ID. Volume overflow returns an error; it does not wrap or delete records.

Archive calls return an error when eligible records exist without an archive backend.
They never claim archival delivery or increment an archive-success count. Encryption,
compression, durable storage, immutable status-change history, export-format assurance,
and archive delivery remain unimplemented production requirements. The existing status
update API changes a compliance field in memory; it is not an append-only audit log.

## Batch identity and readiness

Merging a transaction returns the receiving batch's ID. A new ID is returned only
when a new batch is created. Zero maximum batch size rejects admission. A batch becomes
ready when it is full or its monotonic age reaches the configured timeout. Wall-clock
changes do not control readiness. The default size remains ten and the default timeout
remains 100 milliseconds.

The consumer acquires the metrics lock before it removes a batch. Cancellation while
waiting cannot lose the batch. Removal and request-count updates have no asynchronous
suspension point between them. This does not make delivery durable or acknowledged.
Queue capacity, duplicate handling, retries after delivery, persistence, and service
fallback policy require separate release decisions and implementation.
