# Admission and nonce batch: 9 September 2026

The selected node now uses the execution fee calculation during admission. It reserves the
fee once in DRT and reserves transfer funding in the transfer token. Mainnet is not ready.

The public submission path converts whole-token aliases and base-unit aliases consistently.
It rejects conversion overflow and permits future nonces to reach deferred admission.
The queue derives ready transactions from committed nonces. It does not infer consumed
nonces from rejected or evicted hashes. Selection preserves each sender's nonce order.

Checked reservation sums replace saturating arithmetic. Candidate publication prevents
failed admission from changing the live queue. Rebuilt indexes keep reservations and byte
counts aligned with retained entries. The full u64 gas-price domain orders deterministically.

The [contract](admission-contract.md) defines units, requirements, queue invariants,
compatibility changes, arithmetic reasoning, and qualification limits.

## Verification

Twenty-six of 27 final checks passed. The full workspace test command failed.
The same 120 focused test executions passed in both debug and release builds.
These include 50 admission and queue tests in each build. Ten shared-storage tests passed.
The full workspace recorded 757 passed executions, 14 failures, and one ignored test.
The 14 failed test names match the previous block-settlement batch. They include the two
queue performance and concurrency checks. Matching names do not qualify performance.

The release binary build, default workspace build, isolated genesis/key/profile startup,
formatting, module policy, module-policy tests, and workflow syntax checks passed.
Clippy completed with warnings. No new warning identifies the shared cost module or
new admission tests. The source and test changes passed the final diff check.
Exact commands, output, and failure names are retained in the evidence package.

Fifteen new functional tests cover fee equality with execution, independent fee-token
funding, exact reservations, overflow, self-transfer peaks, sender consistency, aliases,
legacy fee selection, nonce order, rejection, charged failure, eviction, exhausted nonces,
and actual public submission. Existing fixtures now supply DRT fee funding explicitly.
Capacity and ordering assertions distinguish retained entries from executable entries.

A separate investigator completed pre-patch source review. The candidate-review task was
blocked by an automatic cybersecurity filter and returned no review. The parent completed
a local functional consistency pass. Independent assurance remains missing.

## Remaining work

Fix-workflow outcome: `blocked` for release qualification. The original partial security
scan remains sealed. No new full scan or independent candidate review completed.

Complete DRT supply accounting and vesting. Preserve and qualify signed envelopes through
conversion, storage, gossip, and replay. Establish signer-to-address binding evidence.
Resolve allocation, timing, fees, burns, precision, consensus, and launch-module decisions.
Integrate the approved adaptive journal into monetary block settlement after calibration.
Qualify queue performance, persistent admission recovery, all-module state commitments,
distributed consensus, cryptography, operators, migration, and the frozen launch candidate.

All mainnet gates remain open. Local test counts do not define a launch completion
percentage. No remote push, deployment, or mainnet activation occurred. The repository
still lacks original upstream ancestry; do not force-push this restored history upstream.
