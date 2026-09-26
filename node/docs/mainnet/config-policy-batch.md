# Configuration and signature policy corrections: 9 September 2026

Outcome: the configuration boundary now rejects missing credentials and malformed
explicit settings. Debug output no longer includes configuration credentials. The
mainnet release gate remains open.

## Source findings and correction

Previously, secret lookup failures left placeholder API and JWT credentials in
`NodeConfig`. The loader returned success without calling validation. Its derived
Debug output included both credentials and the credential-bearing database URL.
The loader also discarded malformed settings and parsed a proposed signature
allowlist against the old allowlist.

The correction uses one loader validation boundary. It requires credentials, distinguishes
absence from provider failure, parses explicit values, and replaces an algorithm list
only after all entries parse. Debug output redacts credentials. PostgreSQL construction
uses URL components so reserved characters retain their intended meaning.

The secret manager change is necessary for that boundary: its old behavior converted
provider errors and timeouts into absence. The environment provider also treated an
empty value as absent. Both representations now remain distinct from a missing value.
Provider fallback order remains unchanged. The default signature policy remains
Dilithium3 only.

The [implementation contract](config-policy-contract.md) records compatibility changes,
validation limits, provider semantics, and the separate runtime integration requirement.
The source changes are in `config.rs`, `policy/signature_policy.rs`,
`secrets/manager.rs`, and `secrets/providers.rs`. The workflow adds focused checks.

## Verification

The same 39 unique focused tests pass in debug and release. Each mode executes 43
tests because four secrets configuration tests match two filters. Eleven new tests
cover the corrected boundary. Four previous configuration and policy failures now pass.

Every default workspace target builds. The full workspace run completes with 652 passed
executions, 16 failures, and one pre-existing ignored PQClean metadata check. The ignored
check is not a cryptographic known-answer test. No test stalled. The remaining failures
match the prior unresolved failures after removal of the four corrected cases.

Clippy completes with warnings in unchanged code. The modified Rust files have no
reported Clippy warnings. Format, diff, workflow syntax, module policy, and its four
tests pass. No dependency or lockfile change was required.

The evidence package lists every command, exit status, and remaining failed test name.
Focused commands use `cargo test --locked -p dytallix-node` with `--lib config::tests`,
`--lib policy::signature_policy`, `--lib secrets::`, and `--test secrets_integration_test`.
The same filters also run with `--release`. The workspace test uses
`--workspace --all-targets --no-fail-fast`, so unresolved failures remain visible.

The tests use synthetic credentials and local fixtures. They verify that missing,
blank, placeholder, and stub credentials produce errors. Captured loader logs contain
no fixture credentials. Both Debug formats redact the same fields. Invalid settings
and failed providers cannot produce a successful configuration through the strict loader.

Legitimate fixtures still load. Explicit wider PQC lists remain available. A later
provider still supplies a value after an earlier failure. Database credentials with
reserved characters remain in the intended URL components. No database server is contacted.

A fresh Codex Security investigator reviewed the boundary before editing. A separate
fresh reviewer found one remaining invalid-log-level path. Its new regression assertion
failed before the final correction and passed afterward. The log-capture test runs in
an isolated child test process because tracing callsite state is shared across threads.

This is a bounded engineering fix using the Codex Security fix workflow. The original
partial scan remains sealed. This batch does not establish a completed repository scan.

## Mainnet work that remains

The node executables do not consume this loader. Mainnet requires one approved
configuration and signature policy across node startup, transaction admission, and
consensus. Field validation does not prove provider assurance, credential entropy,
TLS enforcement, key custody, or operational readiness. Vault remains a stub.

Genesis units, canonical amount encoding, explicit vesting time, and checked arithmetic
need correction against a reviewed unit contract. This batch does not choose a decimal
scale or rescale balances to make tests pass. Adaptive allocation, atomic token settlement,
calibrated emission assumptions, distributed consensus, independent assurance, operator
qualification, and launch signoff remain open.

No remote push, deployment, or mainnet activation occurred. Test counts do not measure
mainnet completion.
