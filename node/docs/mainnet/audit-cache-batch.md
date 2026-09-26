# Audit, cache, and batch corrections: 9 September 2026

Five previously failing tests now pass. The changes also correct stale cache values,
map-key aliasing, audit visibility, and batch identity. These results do not close the
mainnet release gate.

## Changes

- Added cache sessions bound to exclusive storage access. Unbound reads consult storage.
- Updated cached values at capacity and invalidated removed keys.
- Used canonical Map keys and JSON values without unqualified duplicate writes.
- Encoded complete map batches before writing their first item.
- Corrected access frequency to use the full observation interval and handle clock reversal.
- Made accepted audit entries, staging, and counters one atomic state update.
- Made pending audit records visible to queries and compliance-status updates.
- Rejected disabled or full audit recording without issuing false success IDs.
- Rejected archive completion when no durable archive backend exists.
- Implemented address filtering, report breakdowns, and checked volume arithmetic.
- Returned the actual batch ID for merged requests and used a monotonic timeout.
- Preserved a queued batch when its consumer is cancelled while waiting for metrics.
- Added the focused checks to continuous integration.

The [implementation contract](audit-cache-contract.md) records compatibility changes,
state guarantees, and remaining limits. No package dependency or lockfile change was
required.

## Test expectation corrections

The write-through cache test now uses a session and expects warm reads after writing.
A separate counted-storage test verifies actual backing-read reduction and disabled
caching. The analyzer fixture now supplies four reads per write to satisfy the existing
strict threshold. A separate boundary test confirms that three reads do not satisfy it.

The batch fixture explicitly requests size two before it expects a two-item batch to
be ready. Production defaults remain unchanged. Another test verifies that a partial
batch waits and that wall-clock changes do not override monotonic readiness.

The report test permits a zero-millisecond duration for work that completes in less
than one millisecond. It checks that the reported duration does not exceed the measured
outer duration. No artificial delay was added to make the test pass.

## Verification and remaining work

The same 35 focused tests pass in debug and release mode: 14 storage tests, nine audit
tests, nine optimizer tests, and three report API tests. Eighteen new regression tests
cover the new behavior. They use local state and local fixtures.

Every default workspace target builds. The full workspace test run completes with
637 passed executions, 20 failed executions, and one pre-existing ignored metadata
check. The ignored PQClean metadata check is not a cryptographic known-answer test.
The selected-node release build also passes. Clippy completes with existing warnings;
this is not a warning-free lint gate. Format, diff, workflow syntax, module policy,
and its four tests pass.

The remaining failures are one contract gas-analysis test, eight selected-node tests,
nine core library tests, one core binary test, and one signed-response summary test.
These are seven failing targets. The evidence package records every failed test name.
Tests are evidence of local behavior; counts do not measure mainnet completion.

The next work must resolve the remaining configuration, genesis, signature-policy,
legacy circuit, signed-response, governance, reward-rounding, performance, and gas-analysis
failures. Then repeat the workspace release checks.

Audit staging remains in memory. Durable audit and status-change records, caller error
handling, encryption, archive delivery, and retention enforcement remain open. Cache
sessions require ownership discipline. The new batching behavior does not provide
persistent delivery or an acknowledged work queue.

Adaptive allocation, atomic token settlement, calibrated emission proof assumptions,
distributed consensus, independent assurance, operator qualification, and launch signoff
remain open. No push, deployment, mainnet activation, or new completed security scan
occurred.
