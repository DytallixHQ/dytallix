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

## M-b: the process tests

Three ignored tests start real application processes:
`cross_binary_compat_tests` (two tests: process replacement preserves
history and the index; the candidate manifest binds the running executable)
and `release_handover_process_tests` (a signed handover changes the
executing candidate). As written they could not run:

1. The application requires an owner parent (`ownership::admit`); the
   tests started it directly, so it exited before answering.
2. They did not pass `--release-manifest-sha512`, which the admission
   context must match.
3. A non-test application build refuses the historical snapshot launch.
   The observed verifier it requires needs the four AppArmor roles.

Decision (P01, 28 September 2026): a test build of the application; the
hardened launch stays with E02 and T03.

- **Test build.** The non-default node feature `test-snapshot-verifier`
  accepts the snapshot launch (the same gates as `cfg(test)` in
  `root_genesis.rs`), prints a startup error on stderr, and adds
  `"test_build":"test-snapshot-verifier"` to every response. The bridge
  refuses unknown response fields (`TestChildRejectsTestBuildResponses`), so
  a test build cannot serve a real engine; the process tests assert the
  marker on every response.
- **Owner launcher.** `release-runtime-owned-launch` (release-runtime,
  feature `qualification-fixtures`) runs one program as an owned
  application child from its main thread, passes its standard streams
  through and exits with the child's status. The tests start each
  application through it (`DYT_OWNER_LAUNCHER`) with the root
  authorization's release as the admission context.
- **Two builds.** The runner builds the application twice: once as is and
  once with the node crate at opt-level 1, so the bytes differ and the
  dependencies are shared. This qualifies the replacement and handover
  mechanics; compatibility with an earlier release needs that release as the
  baseline.
- **Runner and CI.** `scripts/run_signed_fixture_tests.py --process systemd`
  runs the three tests through `sudo systemd-run` with no_new_privs and the
  production system call deny list, and fails unless exactly 3 pass. The
  node CI job runs it after the in-process tests. `--process none` runs
  them where both already apply, such as a container started with
  `--security-opt no-new-privileges`.

## Steps

| Step | Content |
| --- | --- |
| M-a | Test-only tools, runner, CI step, phase B drift in four tests |
| M-b | Test application build, owner launcher, process tests in CI |
