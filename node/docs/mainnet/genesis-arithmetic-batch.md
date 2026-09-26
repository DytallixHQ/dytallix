# Genesis arithmetic and import corrections: 9 September 2026

This batch corrects checked vesting arithmetic, exact genesis amount encoding, and
one-time allocation import. Token precision remains unresolved. The batch does not
establish mainnet readiness.

## Changes and evidence boundary

The original vesting function multiplied full-width amounts directly and added
unsigned timestamps without validation. The new calculation uses quotient and
remainder terms. The [arithmetic contract](genesis-arithmetic-contract.md) proves
that the result equals the rational formula rounded down, fits u128, stays monotone,
and conserves vested plus locked amounts. Instant unlock behavior remains unchanged.

Checked allocation validation rejects ambiguous duplicate addresses, invalid schedules,
and overflowing totals. Account creation, runtime construction, and genesis block
validation call configuration validation. Their callers now propagate errors.
Explicit-time query methods replace wall-clock dependence in the affected tests.

Genesis amounts now serialize as strings. A fresh reviewer found that the first
candidate rejected previously accepted numeric literals above u64::MAX. The added
regression failed before correction. The final raw-JSON adapter preserves full-width
integer input without floating-point conversion. Binary amount encoding remains
compatible with the previous representation. The core enables serde_json's raw_value
feature; no package version or lockfile change is required.

The direct storage consumer now parses those amounts exactly. It rejects malformed
or duplicate allocations before any account or metadata write. A synchronous database
batch writes all initial accounts and metadata with a document-digest marker. Reopening
at height zero preserves existing balances instead of importing the allocation again.
Existing nonempty databases without this marker require a reviewed migration. No
migration, database deletion, or existing-database rewrite occurred.

The genesis file-generation test now writes into a private temporary directory. It no
longer writes a shared genesisBlock.json outside the repository. The example prints raw
amounts and identifies outstanding requirements instead of claiming mainnet readiness.

## Verification

All 28 focused tests pass in debug and release: 15 genesis tests, eight storage tests,
two explicit integration checks, and three example tests. Fifteen unique regression tests
were added. The full workspace executes 28 additional test cases because the core binary
also includes the genesis and storage test modules.

Every default workspace target builds. The full workspace test completes with 682 passed
executions, 14 failures, and one pre-existing ignored PQClean metadata check. That ignored
check is not a cryptographic known-answer test. No test stalled. The two amount-serialization
failures now pass; the other previously failing cases remain visible.

Clippy completes with warnings in unchanged code. Format, diff, workflow syntax, module
policy, and its four tests pass. The evidence package lists each command and exit status.
Focused checks use `cargo test --locked -p dytallix-node`, the relevant library filters,
and `--example genesis_usage`; the same checks run with `--release`. The full workspace
check uses `--workspace --all-targets --no-fail-fast`.

Tests cover full-width amounts, timestamp boundaries, instant unlock, invalid domains,
monotonicity, conservation, parser compatibility, duplicate allocations, import failure
without partial state, restart at height zero, changed file rejection, and legacy database
rejection. The exact formula proof covers the admitted arithmetic domain. The finite
tests supplement that proof; they do not establish economic stability.

A fresh read-only investigator traced direct callers before implementation. A separate
fresh reviewer checked an immutable candidate patch. The final package includes that
candidate and the failed large-integer regression before the correction. The current
patch and source archives identify the final implementation separately.

Three existing integration assertions still compare the unchanged raw allocations
against a different decimal scale. Their expected amounts remain unchanged. The workspace
workflow still runs those tests. The new focused checks do not hide the release failure.

This is a bounded Codex Security fix workflow. The original partial security scan remains
sealed. No new completed repository scan, remote push, deployment, or mainnet activation
occurred.

## Next mainnet work

Resolve token precision before rescaling amounts or changing supply limits. Then unify
the genesis representation and supply accounting used by the selected node, core runtime,
and persistent storage. The core runtime still seeds an extra default balance, leaves its
supply counter inconsistent, and does not fund genesis stake from matching balances.
Those defects remain open and are not covered by this batch's arithmetic closure.

Complete vesting claim/spend accounting, genesis block commitment, validator key checks,
atomic token settlement, and the selected adaptive model's approved allocation and timing.
Distributed consensus, independent assurance, operator qualification, migration, and launch
signoff remain required. Test counts do not measure mainnet completion.
