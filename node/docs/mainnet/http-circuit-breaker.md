# HTTP circuit-breaker contract

This control belongs to the core AI HTTP client. It controls service availability.
It does not authenticate responses or establish consensus validity.

## Construction and scope

Use `AIOracleClient::with_circuit_breaker(service_config, circuit_config)` to enable
one circuit for that client. Share the client through `Arc` to share the circuit.
Health requests and all POST endpoints use the same circuit. Existing calls to
`AIOracleClient::new` leave the circuit disabled. The consensus constructors still
use `new`; this batch does not select or activate a production circuit policy.

The new types live in `consensus::http_circuit_breaker`. They do not reuse the old
public `CircuitBreakerContext`, which has different semantics and remains separate.

The caller must supply these settings:

| Setting | Accepted value |
|---|---|
| Failure threshold | 1 to 10,000 basis points; 5,000 means 50% |
| Window size | 1 to 65,536 completed requests |
| Minimum sample count | 1 to the window size |
| Recovery interval | A nonzero Duration; subsecond precision is retained |
| HTTP attempts and timeout | Both must be nonzero at circuit construction |

These constraints validate representation and operation. They do not establish
suitable production settings. The example's values are demonstration values.

## Request outcomes

A health request succeeds when its HTTP status is successful. Every other HTTP
status counts as a failure, including client-error statuses. A transport error or
timeout also counts as a failure. Health keeps its existing return contract:
non-success status returns `Ok(false)`; transport failure returns an error.

A POST succeeds only after a successful HTTP status and successful JSON decoding.
A logical POST request can include the existing server-error retries. It contributes
one final outcome to the circuit. Transport errors keep the existing behavior and
return immediately. A recovery probe gets one HTTP attempt, even if normal POST
requests permit retries. An open circuit returns an explicit error before HTTP.

The analysis transport remains unfinished. `request_analysis` now returns an
explicit error. It no longer fabricates a low-risk result with an empty signature.
This change does not supply an analysis service contract or signature verification.
The integration manager still has a separate `fail_on_ai_unavailable` policy.
Its default permits fallback. This batch does not qualify that policy for mainnet.

## State and concurrency

- Closed: admit requests. Store completed outcomes in a fixed-size sliding window.
- Open: reject new requests until the recovery interval has elapsed.
- Half-open: admit exactly one recovery probe. Reject other requests during that probe.
- Successful probe: close the circuit and clear the sample window.
- Failed or cancelled probe: reopen the circuit for a full recovery interval.

Recovery uses a monotonic clock. Reading status does not start a probe. A mutex
serializes state transitions. No HTTP request or asynchronous wait holds that mutex.
Requests admitted before the circuit opens can still complete and have remote effects.

Let `n` be the sample count, `f` the failure count, and `T` the threshold in basis
points. Open the circuit when `n >= min_requests` and `10,000*f >= T*n`.
The comparison uses integers. At the maximum window size, each product is at most
655,360,000. The implementation uses u64 products. This comparison does not overflow.
When the window is full, remove its oldest outcome before adding the next outcome.

Each request captures identity tokens for its reset period and circuit generation.
Results from an earlier generation update lifetime counters but cannot change the
current state or sample window. Results from before an explicit reset do neither.
The tokens use reference-counted identity, so no integer generation counter can wrap.

Cancelling a closed-state request increments the cancellation counter. It does not
assert a service failure or add a sample. Dropping a recovery request reopens the
circuit. This releases the probe slot even when a task is aborted. If the state mutex
is poisoned, admission returns an error; the cancellation path does not panic.

## Statistics and reset

Status includes admitted, rejected, successful, failed, and cancelled request counts.
It also includes current window counts. Lifetime counters saturate at u64::MAX.
The window stores only closed-state outcomes. Probe outcomes affect lifetime counters
and state. Recovery preserves lifetime counters. Explicit reset clears all counters and returns
to Closed. Reset neither cancels earlier requests nor reverses their remote effects.

## Limits and integration work

The circuit bounds sample storage and recovery-probe concurrency. It does not bound
normal in-flight requests, response bytes, or the total time across retries and delays.
The HTTP timeout remains a per-attempt timeout. Retried POST operations still need
an agreed idempotency contract before use for operations with side effects.

Local availability controls must not become validator-dependent consensus decisions.
Select the analysis protocol, verification boundary, fallback policy, request budgets,
and production circuit settings before enabling the service in a release profile.
The selected fast-node runtime and adaptive emission settlement do not use this circuit.
