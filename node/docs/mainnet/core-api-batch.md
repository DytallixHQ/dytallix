# Core API repair batch: 9 September 2026

## Runtime correction

Changing the AI HTTP client's timeout previously changed its configuration only.
Later requests still used the timeout set when the client was created.
The health and POST paths now apply the current timeout to each HTTP attempt.
This is an attempt timeout, not a total budget across retries and delays.

Two local tests failed before the correction. Both set the timeout from five
seconds to one second against a fixture that responds after three seconds.
Before the correction, both requests succeeded after the delay. After the
correction, both return a transport timeout. No public test service is required.

The health tests now use the actual API: successful HTTP status returns true,
unsuccessful status returns false, and transport failure returns an error.
The health example uses that same contract. A successful HTTP status does not
establish oracle authenticity or correctness of an analysis result.

## Test and build repairs

- Replaced obsolete signature-test types and mock byte arrays with current types,
  generated Dilithium3 keys, and real signatures. Assertions cover successful
  verification, an absent transfer signature, nonce reuse after successful
  verification, unregistered oracles, registry updates, and manager statistics.
- Corrected the block-signing test imports. The unsigned header fixture now
  declares Dilithium3, which matches the current manager. The algorithm field
  remains part of the signed header representation. Runtime cryptography did not change.
- Converted 14 oracle registration calls to RegisterOracleArgs. Stake fixtures
  now use the current u128 amount type. Fixture amounts did not change.
- Corrected the response-count expectation. Of five responses, three are accurate
  with valid signatures, one is inaccurate, and one has an invalid signature.
  The test also checks that those disjoint counts sum to the total.
- Removed the duplicate aggregate test that called ten native test wrappers
  from inside another asynchronous runtime. All ten independent tests remain.
- Added these four repaired test targets to continuous integration before the
  complete workspace suite. No target was ignored or feature-gated to hide a failure.

## Verification

| Check | Result |
|---|---|
| Health HTTP tests | 8 passed |
| Core signature integration | 8 passed |
| Core block-signing tests | 2 passed |
| Oracle registry tests | 10 passed |
| Debug and release checks | Same 28 unique tests pass in each mode |
| Health example | Compiles in both modes; no external service was queried |
| Selected-node release binary | Builds successfully |
| Selected-node test suite | 212 passed, 8 failed, 0 ignored; same failure set as the prior recorded suite |
| Format, diff, workflow syntax, module policy | Passed; includes 4 policy tests |

The default core build inventory used:
`cargo build --locked -p dytallix-node --all-targets --keep-going`.
It reached all default build targets and reported two failing targets:

- `circuit_breaker_test`
- `circuit_breaker_demo`

Both refer to HTTP circuit-breaker APIs that the active client does not implement.
The complete core/workspace build gate remains open. The core library lists 125
unit tests, but this batch did not execute that entire library suite.

## Assurance limits and next work

These crypto tests exercise the current core Dilithium3 implementation. They are
not ML-DSA conformance vectors or independent cryptographic review. Signature
fixtures disable certificate-chain checks explicitly. Oracle registry tests
exercise metadata and policy counters, not cryptographic validation of their keys.

The block transaction check does not integrate AI verification. The core AI
client's analysis method still creates a placeholder response. This batch did
not enable either path for mainnet or qualify these behaviors for launch.

Implement and verify the HTTP circuit-breaker contract next. Define recovery,
failed probes, request limits, and cancellation. Preserve explicit failure states;
an unavailable service must not produce an accepted analysis result.
Then execute the complete core and workspace suites and resolve their failures.

The selected-node governance, reward-rounding, and performance failures remain
open. Adaptive allocation, atomic supply settlement, calibration, distributed
consensus, independent review, and operator qualification also remain open.
No mainnet activation, remote push, deployment, or launch approval occurred.
