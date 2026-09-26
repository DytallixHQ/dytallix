# Block settlement batch: 9 September 2026

The selected node now plans and commits a supported development block in one synchronous
database batch. Mainnet is not ready. Release validation remains blocked by existing tests.

## Changes

The producer stages transactions, withheld fees, inactivity switches, emission, and optional
staking accounting before writing. One commit includes block contents, receipts, indexes,
height, policy, and recovery head. Caches and notifications follow the successful commit.
Charged failures remain in the block. Pre-fee rejections receive terminal receipts without
fees or block positions. Empty skipped ticks do not advance emission.

Block headers bind transaction bodies, ordered assets, receipts, request inputs, and selected
state. Recovery verifies history and current state. Matching last-block retries do not repeat
fees or emission. Tests cover a failed write and a lost acknowledgement after a complete write.

Queue admission becomes visible only after durable Pending records exist. Failed admission
preserves the live queue. Late Pending writes cannot replace terminal receipts. Asset
acknowledgement retains entries added after the producer took its snapshot.

The startup profile must explicitly select development. Mainnet remains unavailable.
Governance and direct funding activation fail. Direct emission and staking mutation routes
remain disabled until they use signed block transitions. Staking reads and staged emission
remain available. Invalid explicit emission configuration fails instead of selecting defaults.

The [implementation contract](block-settlement-contract.md) defines the boundary, arithmetic
identities, development policy, compatibility changes, recovery checks, and limitations.
The existing corrected adaptive controller remains separate and inactive. No mainnet model,
allocation, timing, token precision, fee allocation, or burn decision changed.

## Verification

All 70 focused test executions pass in both debug and release. This includes 21 new block
tests, one new staking-route test, one revised emission-route test, and the existing related
transaction, gas, fee, and producer-control tests. Ten shared-storage tests pass.

The selected release builds. Isolated genesis, existing-key, and profile startup checks pass.
These checks stop before signing or RPC startup. All default workspace targets build.
The full workspace completes with 742 passes, 14 failures, and one existing ignored check.
The failure names match the previous batch. The ignored check is not a cryptographic
known-answer test.

The 14 failures remain in contract gas analysis, mempool performance, stake-weighted
governance, staking reward rounding, the core circuit breaker, core genesis assertions,
and signed-response summary handling. Clippy completes with existing warnings. Formatting,
patch whitespace, module policy, module-policy tests, and workflow syntax checks pass.
The evidence package records all 25 final commands and exit statuses.

The independent candidate review found two defects. Enabled staking routes could write
outside the block. Rejected transactions could retain Pending receipts. The final patch
closes both paths. The tests check disabled mutation routes with staking enabled and terminal
rejection persistence after a permitted inactivity-switch transition and database reopen.

The former direct emission-claim integration test now checks the deliberate route rejection
and unchanged balance and pool. Other existing gas and transaction assertions remain intact.
The new route test retains its temporary database for the full test lifetime.

## Remaining work

Fix-workflow outcome: `blocked` for release validation. Focused tests establish the changed
local behavior. They do not close a mainnet gate or establish whole-chain security coverage.
The original partial Codex Security scan remains sealed. This batch did not run a new scan.

Next, reconcile mempool fee reservation with execution, complete DRT supply and vesting
accounting, and add signed module transitions. Resolve adaptive allocation and timebase,
fees, burns, precision, consensus, and launch-module decisions before monetary activation.
Connect the approved adaptive journal to the block batch after those decisions and controller
calibration. Replace full-history verification per block with a qualified incremental design.

Pending queues remain volatile. Legacy histories require explicit migration. Full state and
history scans have increasing cost. The selected digest excludes contracts, bridges, oracles,
and other module state. No physical power-loss test, distributed consensus test, independent
operator trial, or final launch approval occurred. No mainnet percentage follows from local
test counts. All mainnet acceptance gates remain open.

No remote push, deployment, or mainnet activation occurred. The restored repository lacks
original upstream ancestry. Apply reviewed patches onto original history; do not force-push
this restored branch over upstream history.
