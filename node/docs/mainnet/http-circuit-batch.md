# HTTP circuit-breaker batch: 9 September 2026

The core HTTP client now supports an explicit circuit-breaker policy. Both previously
broken circuit-breaker targets compile. The complete default core build inventory
also passes. The complete test gate remains open.

## Changes

- Added private circuit state, a bounded sample window, and an integer failure threshold.
- Connected circuit admission and outcomes to real health and POST requests.
- Limited recovery to one concurrent probe and one HTTP attempt.
- Added cancellation handling and protection against results from earlier circuit states.
- Added status counters and reset behavior that ignores requests admitted before reset.
- Removed the fabricated analysis response. The unfinished analysis transport returns an error.
- Replaced external connectivity tests with bounded loopback fixtures and explicit assertions.
- Updated the circuit example and continuous integration checks to use the implemented API.

The [HTTP contract](http-circuit-breaker.md) specifies the state transitions,
threshold arithmetic, scope, and remaining integration requirements.
No new package dependency or lockfile change was required.

## Verification

| Check | Result |
|---|---|
| HTTP circuit integration | 16 tests passed |
| HTTP connectivity | 6 tests passed |
| Existing health, signature, block-signing, and registry checks | 28 tests passed |
| Debug and release focused checks | Same 50 unique tests passed in each mode; none ignored |
| Health and circuit examples | Compile in both modes; no external service was queried |
| Complete default core build | Passed for all targets |
| Selected fast-node release binary | Builds successfully |
| Clippy | Completes; existing warnings outside this batch remain |
| Full default core test attempt | 212 passed, 16 failed executions, 4 stalled tests; none ignored |
| Format, diff, module policy, workflow syntax | Passed; includes 4 module-policy tests |

The full core test attempt used `--no-fail-fast`. Four library tests remained stalled
when the library process was terminated after 100 seconds. Cargo then ran all
remaining targets. The four stalled tests are review-queue workflows. Their methods
hold the transaction write lock and call `update_stats`, which requests the same
transaction read lock. This source did not change in this batch.

The full attempt preceded two additional circuit tests for adjacent integer thresholds
and reset during a cancelled probe. A later lint correction combined two nested
conditions without changing the window update rule. The final focused debug and release runs include
both tests. The full-suite totals above report the original attempt without adding
results from later runs.

A separate worktree at base revision `425121d2652d9617226145ac80caa2e7ee8c0736`
ran the failing targets. All 16 observed failed executions also failed on that base.
The baseline had five additional database-lock failures. Tests share `./data/node.db`,
so scheduling affects which test fails. The baseline comparison skipped the four
stalled tests explicitly. It does not prove that those workflows pass.

The comparison initially left stale baseline build products in the shared Cargo
cache. That caused missing-API compile errors during the first return to the changed
source. Cleaning only the core package cache resolved this verification issue.
The final debug and release checks rebuilt the changed core package and passed.
The evidence retains the intermediate logs and the cache-clean record.

## Remaining work

1. Repair review-queue lock ownership and use bounded tests for each workflow.
2. Give each test its own database and isolate environment changes.
3. Resolve the remaining configuration, audit, compliance, batch-processing, legacy
   circuit-context, genesis, signature-policy, and response-summary failures.
4. Rerun complete core and workspace tests after those corrections.
5. Define the analysis service contract, verification boundary, fallback behavior,
   and request budgets before enabling the circuit in a release profile.
6. Complete the adaptive emission allocation and atomic settlement design. Then
   calibrate the model and complete independent mathematical and economic review.

The selected corrected adaptive model remains unchanged. Its bounded controller
and command journal do not mint tokens. Allocation approval and atomic settlement
remain open. The conditional stability proof does not establish market stability
without its plant assumptions and calibration evidence.

The circuit remains opt-in. Existing consensus constructors do not enable it.
Removing the fabricated result does not remove the integration manager's separate
fallback policy. Local HTTP results do not establish distributed consensus, mainnet
readiness, cryptographic conformance, or operator qualification.

No remote push, deployment, mainnet activation, or new security-scan completion
occurred. The original partial, sealed Codex Security scan remains unchanged.
