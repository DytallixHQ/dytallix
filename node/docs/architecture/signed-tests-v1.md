# Signed migration and handover tests (E04 gap 11)

Engineering task E04, gap 11 of the [E04.1 triage](../mainnet/e04-requirement-triage.md)
(UPG-003). No policy decision: this is test infrastructure. The tests
verify real SLH-DSA-SHAKE-256s signatures made with disposable fixture keys;
no production key or value is involved.

## Problem

The tests that exercise signed root genesis, emergency controls, upgrades,
index migration, release handover and history replay were `#[ignore]`.
They needed a verifier, signers and public fixtures that the repository did
not build, so they had not run on the phase B state layout (the state tree
root, derived epoch observations).

## M-a: the in-process tests

- **Tools.** Three test-only tools in `consensus/root-authorization`:
  - `cmd/dytallix-root-verify-snapshot`: `root.VerifyRequest` over the whole
    request on stdin, without the owner guard. It serves the node's
    historical snapshot launch (`helper_execution: None`), which
    `root_genesis::validate_helper_execution` accepts only in test builds.
    Production runs `dytallix-root-verify` under the observed owner
    protocol.
  - `cmd/dytallix-fixture-sign`: signs a test artifact for an emergency or
    upgrade action with a key derived from one public byte
    (`--fixture-key`). Every key it can produce is public.
  - The root-authorization test binary, whose
    `TestExportDevelopmentGenesis` signs development genesis bundles.
- **Runner.** `scripts/run_signed_fixture_tests.py` builds the tools, signs
  the three public fixtures (emergency, upgrade, and a second emergency key),
  sets the `DYT_*` variables and runs the ignored node library tests. It
  fails unless exactly 21 pass, so a renamed or filtered test cannot drop
  out silently. `--tools DIR` uses prebuilt tools on a host without Go.
- **CI.** The node job runs the runner after the workspace tests.
- **Drift fixed on the phase B layout.**
  - Two tests built an epoch observation by hand (`utilization_ppm:
    500000`). Observations are now derived from committed block space and a
    submitted one is refused, so the tests use `derived_observation_wire`
    and expect CheckTx to refuse a submitted copy.
  - The handover startup test forges a structurally consistent history with
    a wrong signature. The state root is now the state tree's, so the test
    drops the tree records and rebuilds the tree over the forged state
    (`state_tree::rebuild`), as a restored snapshot would.
  - The bootstrap scratch test expected the old error text. The history
    verifier now refuses the missing scratch path first
    (`EmergencyVerifier::new`); the helper still does not run and the
    database is unchanged.
- **Result.** 21 of 21 pass on macOS and Linux.

## M-b: the process tests (not yet run)

Three ignored tests start real application processes:
`cross_binary_compat_tests` (two tests: process replacement preserves
history and the index; the candidate manifest binds the running executable)
and `release_handover_process_tests` (a signed handover changes the
executing candidate). They cannot run as written:

1. The application now requires an owner parent (`ownership::admit`); the
   tests start it directly, so it exits before answering.
2. They do not pass `--release-manifest-sha512`, which the admission context
   must match.
3. A non-test application build refuses the historical snapshot launch, so
   the root verifier must run under the observed owner protocol, which needs
   AppArmor role labels.

M-b moves them into a Linux qualification harness that owns each process
from its main thread (as `state-sync-join` does) and runs in the E02 native
job, which has AppArmor and root. The two application builds must have
distinct bytes; a debug and a release build of the same source are enough
for the mechanics.

## Steps

| Step | Content |
| --- | --- |
| M-a | Test-only tools, runner, CI step, phase B drift in four tests |
| M-b | Process tests in an owned Linux harness in the E02 native job |
