# Next integration batch

## Restored source

The source now resides in a persistent checkout under the user's Developer
folder. Archive checksums were checked before extraction. The restored node Git
tree matches the saved tree for commit 406670d890dfe077d95b8ddef4fc00ad7f264776.

The source archives contain no Git history. Each restored repository has a new
snapshot commit. RESTORED-SOURCES.json outside the repositories maps those commits
to the original revisions and source digests. Do not push the new root history as
an upstream replacement. Apply the reviewed patch to the original source branch
when upstream history is available.

## Deferred transactions

The sender/nonce index now stores each deferred transaction's hash. Closing a
nonce gap can therefore promote the stored transaction. A second deferred
transaction with the same sender and nonce receives a policy rejection before
capacity or reservation changes. This prevents an index overwrite.

Tests require all 50 transactions in the existing reverse-arrival comparison to
be admitted. Additional tests check reservation retention and release, index
removal, conflicting nonce rejection, and promotion at capacity. These tests do
not qualify every eviction, replacement, or finalized-state lifecycle.

## Registry fixture

The registry test now builds a small Rust contract from source with the pinned
compiler and lockfile. CI builds it before running workspace tests. The test
checks two asset registrations, retrieval of the first asset after the second,
and an unknown asset. The generated artifact is a build prerequisite, not a
mainnet contract deployment.

Caller authentication, contract isolation, node database persistence, restart
recovery, and deterministic runtime metadata remain open. The test explicitly
records the current placeholder caller.

## Core test/API repairs

- Use the current metadata and request constructors. Check request round-trip
  serialization and defaults. Remove the obsolete external httpbin request;
  the old test did not assert its response and its client method is unavailable.
- Remove a redundant manual runner that awaited synchronous wrappers generated
  by the test harness. The six original local-model tests remain enabled.
- Correct secrets example imports and logging initialization.
- Use existing genesis helper names and u128 arithmetic in the example. Compare
  unvested allocations with their configured amount. Do not change genesis rules.

These changes do not restore unavailable APIs. Other core tests and examples
still reference obsolete health, circuit-breaker, signature, and block interfaces.
Full workspace compilation remains a required gate. Local-model signature tests
do not establish production cryptographic verification.

## Next order

1. Finish core test/API reconciliation against an approved feature list. Do not
   restore a stub API merely to satisfy an obsolete test.
2. Repair governance and staking fixture ownership. Diagnose reward accrual
   against the selected lifecycle before changing accounting behavior.
3. Approve the decisions in protocol-decision-draft.md.
4. Implement shared fee and supply accounting from that approved specification.
5. Integrate distributed consensus and qualify persistence, recovery, and upgrades.

Performance thresholds were not increased or disabled in this batch.
