# Review-queue and test-isolation batch: 9 September 2026

Review-queue methods now complete without the observed lock cycle. Admission and
review decisions update canonical state and indexes together. Tests now use private
database and key-store paths. The full core test run finishes without stalled tests
or database-lock errors.

## Implemented changes

- Replaced four separately locked queue structures with one private state boundary.
- Made duplicate and active-capacity checks atomic with insertion.
- Removed obsolete pending copies when a record enters review or receives a decision.
- Added one-step selection and claim. Selection alone no longer consumes a record.
- Prevented repeated or conflicting final decisions from changing completed records.
- Made expiry check and update one operation. Moved notifications outside the state lock.
- Recomputed statistics from a current snapshot. Repeated reads no longer inflate counts.
- Added explicit storage and key-store paths without changing default constructors.
- Gave database tests private temporary directories with cleanup after resource release.
- Corrected the initialization test to assert actual statistics counters. The old test
  requested a configuration field that the statistics response does not contain.
- Added queue, review API, storage, and fixture checks to continuous integration.

The [state contract](review-queue-contract.md) records API behavior changes and limits.
No package dependency or lockfile change was required.

## Verification

The focused checks cover 31 unique tests in both debug and release:

| Area | Passed tests |
|---|---:|
| Queue workflows and concurrency | 16 |
| Review API | 3 |
| Explicit storage and persistence checks | 3 |
| Consensus fixtures | 6 |
| Contract fixtures | 3 |

Ten new queue tests cover concurrent duplicate admission, active capacity, unique
claims, selection, immutable final decisions, statistics, expiry, duration bounds,
and cancellation. Each new queue test has a five-second completion bound. The four
previously stalled workflows now complete.

The complete default core run reports 234 passed executions, 14 failed executions,
zero ignored tests, and no stalled tests. Its library target completes in about
0.21 seconds. This is a local test duration, not a production throughput result.
The current full-run total includes newly added tests; it is not a launch percentage.

The remaining core failures cover configuration, audit counts, report timing, batch
processing, the separate legacy circuit context, genesis amount expectations,
signature-policy expectations, and signed-response summary fields. No test was
ignored or feature-gated to hide these failures.

## Workspace result and remaining work

Every default workspace target builds. The full workspace test run completes with
614 passed executions, 25 failed executions, and one pre-existing ignored metadata
check. The ignored check requires a separate PQClean metadata fixture; it is not a
cryptographic known-answer test. The test suite is not green.

The seven failing targets are:

| Target | Failed executions |
|---|---:|
| Contracts library | 3 |
| Selected-node mempool performance | 2 |
| Selected-node stake-weighted governance | 5 |
| Selected-node staking reward accrual | 1 |
| Core library | 12 |
| Core node binary | 1 |
| Core signed-response summary | 1 |

Contract failures concern cache statistics, a storage-analysis recommendation, and
a gas-analysis expectation. The selected-node failures match the previously recorded
performance, governance, and reward-rounding work. Core failures are listed above.
Source inspection must distinguish implementation errors from obsolete expectations
before any assertion or policy changes.

The next engineering batch should resolve audit and cache accounting, batching,
and deterministic fixture expectations. Economic and signature-policy tests must
follow the selected specification. The allocation decision remains pending.
Then rerun the complete workspace suite before release qualification.

The focused checks pass in release mode. The core target build and selected-node release build
checks pass. Format, diff, module policy, its four tests, and workflow syntax pass.
Clippy completes with existing warnings; this is not a warning-free lint gate.

## Assurance limits

The review queue remains an in-memory service. It lacks durable history and durable
notification delivery. Caller authorization, bounded retention, restart behavior,
and production load qualification remain open.

The explicit constructors separate test resources. They do not qualify the core
storage initialization path or key manager for mainnet. The core key manager still
creates placeholder key material and can regenerate keys when loading fails. The
contract fixtures also use mock transactions. Passing these fixtures does not prove
signature validation, contract isolation, or distributed consensus.

The selected fast-node runtime still has separate governance, staking-rounding, and
performance work. Adaptive allocation, atomic settlement, model calibration, independent
review, operator qualification, and mainnet signoff remain open.
No remote push, deployment, mainnet activation, or new completed security scan occurred.
