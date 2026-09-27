# E03 negative-peer method

Status: **E03 engineering complete; the method awaits formal independent
review (P02).** 26 September 2026. The two cross-host cases (wrong-key, then
malformed-peer) are T02. No host was contacted in this step. Mainnet remains
`NO_GO`.

The method packet is kept outside this public repository because it pins
staging hosts and their private network: `decision-register/core-function-alignment/pqc-negative-trial-20260924/e03_staging_candidate_v2/`.
This record summarizes its state against the maintained source.

## Requirement

Integrate the negative-peer sender, receiver observation, supervised
identity and an independent evidence collector into a sealed staging runner,
keeping the earlier V3 and V5.2 results unchanged, and resolve whether the
receiver's final-byte read meets the event-order rule (engineering split,
E03.1–E03.5).

## State

| Subtask | State |
| --- | --- |
| E03.1 Event-order property | Approved by Rick Glenn (`E03_EVENT_ORDER_APPROVAL.json`): the same receiver process reads the complete invalid frame (sender and receiver byte counts and SHA-256 equal for one instance and socket tuple) before its case-specific rejection branch. It does not claim the receiver outlived the sender's later `Write` |
| E03.2 Sender | Records the case instance, accepted-byte count, frame hash and transport-config hash; limited to the wrong-key and malformed-peer cases |
| E03.3 Receiver | One supervised instance records its PID, start ticks, running executable hash, rejection branch and exit |
| E03.4 Collector | Hashes the raw sender, receiver and supervisor records before parsing; binds the socket tuple, interface, endpoints, fixture, timeout and case ID |
| E03.5 Runner | One case per invocation; the malformed-peer case requires a recomputed, separately reviewed wrong-key result; token-owned paths, a narrow firewall rule, evidence salvage before cleanup, exact-rule rollback and afterchecks; any ambiguous remote result stops further mutation. Dispatch is refused without the independent review record |

## Checks against the maintained source (26 September)

- The packet is unchanged since the review request: the runner, build
  manifest, event-order approval and recovery procedure match their
  requested SHA-256 values.
- `host_runner.py --verify-local`: `LOCAL_PINS_VERIFIED`, zero host actions.
  The 15 runner and collector tests (SSH mocked) and the probe's Go tests pass.
- The probe's `internal/pqcp2p` equals the engine's
  `consensus/cometbft/internal/pqcp2p` except for the additive staging-only
  `RejectionObserver` hook (nil outside the probe) and one older error
  message. The rejection logic under test is the shipped logic. The probe uses
  only that package, circl and `golang.org/x/crypto`, so the E01 changes to
  `privval` and `node` do not affect it. The E01 inventory's peer route now
  records that a change there requires refreshing this copy and its review.
- The pinned sender (`135c7d97…`) and receiver (`ce450e3a…`) rebuild
  byte-identically from the packet source with go1.26.0:
  `GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -buildvcs=false -ldflags=-buildid= ./cmd/{sender,receiver}`.
  The build manifest did not record the `-ldflags=-buildid=` flag.

## Remaining

1. **P02:** a formally independent reviewer inspects the exact method against
   the protected-host requirements and, if accepted, records
   `E03_HOST_METHOD_REVIEW.json` with the reviewer, scope, findings and
   `decision: APPROVED_FOR_ONE_BOUNDED_CASE`, bound to the three approval
   hashes. The earlier technical reviews do not satisfy this.
2. **T02:** one bounded wrong-key cross-host case; inspection of its raw files
   and a separate first-case decision; then the malformed-peer case. Each run
   touches shared staging hosts and needs explicit approval.

Any change to the runner, build manifest, event-order decision or recovery
procedure invalidates the review.
