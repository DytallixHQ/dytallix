# Supervisor root preflight (design note, for a P01 decision)

Status: **open**. Found by the first staging install (H4, PR #12, 7 October
2026). No production supervisor can start until this is decided.

## The conflict

Two approved designs disagree about who may run the root helper
(`dytallix-root-verify`, the SLH-DSA verifier).

| Source | Rule |
| --- | --- |
| E02 ([e02-helper-ownership.md](../mainnet/e02-helper-ownership.md)) | Execution edges S→A, S→W, **A→H only**. The application is the helper's single owner (E02.4). The Go guard requires the exact `A//&H//&S` label before READY (E02.5). The rendered profiles say: "A direct supervisor-to-helper exec yields an incomplete stack. The helper must reject every label without the owner component." The supervisor's profile has no ptrace-read or STOP/CONT rights over a helper it would start itself. |
| A5 ([production-activation-v1.md](production-activation-v1.md)) and the [restart runbook](../operations/restart.md) | The production supervisor "opens through the threshold root (`preflight_release_with_root`)", and "the supervisor's preflight verifies the authorization with the root helper and selects the fixed release". |

The code follows both. `native-supervisor` `authorize()` calls
`preflight_release_with_root`, which runs `RootGenesis::prepare` and the
bootstrap history verifier, and so `observed_execution::run`. There,
`HelperSecurityPolicy::expected_child()` requires the caller's label to be
the application owner's: "Application owner role required for helper". The
supervisor's label is S, so every production start is refused before any
child exists.

Nothing caught it earlier, for the same reason as the E02 startup-check
defect: the process tests use the test-only snapshot verifier, and the E02
native checks qualify only A→H. H4 is the first run of the production
supervisor's start path.

## What the supervisor's preflight is for

`authorize()` returns `VerifiedReleaseAuthority`, the verified member files
(the catalog) and the root helper's description. The supervisor needs the
**catalog before it starts any child**: `ProcessOwner::new(catalog, …)` checks
each child's exact file. The catalog is verified against the authority.

The application verifies the same things itself before it opens state. It
gets the root, verifier and candidate configurations and
`--restart-authorization`, runs the helper (A→H) and refuses on any mismatch.
So the supervisor's root verification is a second, independent check on the
same signatures. The supervisor then compares the application's `info` with
its authority (`verify_info`).

## Why simple delegation does not work

"Let the application verify for the supervisor" is circular. To start the
application safely, the supervisor needs the verified catalog. To get the
catalog, it needs the authority, which is what the root verification
produces. The node already guards a close relative of this:
`bootstrap_history_verifier` exists because live settings must not select
the executable that establishes the authority that later authorizes it.

## Options

### A. Supervisor-owned helper (change E02)

The supervisor may start the helper. The launcher accepts an S caller and
expects the stacked label of the supervisor→helper exec (`H//&S` today).

- **Kept:** the supervisor's independent signature check, A5 and the restart
  runbook as written.
- **Changed:** E02.2 adds S→H. E02.5's Go guard accepts the S-owned label as
  well as `A//&H//&S`. The supervisor profile gains ptrace read and STOP/CONT
  for the helper stack. The "owner component required" rule is replaced by
  "an owner component from {A, S}", which needs either a new S-owner component
  in the stack or an argument for why `H//&S` cannot be reached from W.
- **Work:** launcher, Go guard, `production_roles.py`, E02 native checks for
  S→H (start, observe, owner death, deadline), the E02 record.
- **Risk:** a security-model change in a reviewed area. The helper gains a
  second, more privileged owner.

### B. Verify-only application start (keep E02)

The supervisor builds a **provisional** catalog from the operator-installed,
digest-pinned files (`release-manifest.json`, whose SHA-512 the root-owned
`root-config.json` pins), starts the application once in a new verify-only
mode (S→A, then A→H as today), reads the verified authority, and requires it
to match the provisional catalog before starting the node normally.

- **Kept:** E02 unchanged.
- **Changed:** the supervisor no longer verifies the signatures itself; it
  trusts the application's report. The application binary is chosen before
  the signatures are checked (from pinned digests), which the bootstrap rule
  above is wary of.
- **Work:** a new application mode and reply, a second start in the
  supervisor, tests. Startup roughly doubles.

### C. Separate pre-start verifier unit

A oneshot unit with its own account and an AppArmor role (V→H) verifies the
root and writes a receipt that the supervisor reads under its locks.

- **Kept:** E02's A→H rule for the node's own processes; independent
  verification.
- **Changed:** a fifth role and a second unit, a receipt format whose
  integrity rests on file ownership, ordering between units, host setup and
  the installer.
- **Work:** the largest of the four.

### D. Supervisor checks pinned digests; the application verifies signatures (keep E02)

The supervisor stops running the helper. It selects and checks the catalog
against the operator-installed, digest-pinned release manifest and
configuration, as the bundle installer already verifies them. It starts the
application, which verifies the root signatures and any restart
authorization (A→H) before opening state, as it does today. The supervisor
then requires the application's `info` to match the pinned release before
starting the engine.

- **Kept:** E02 unchanged. Every signature is still verified before any
  state opens or any block is made, by the application, which is the
  process that signs and executes blocks anyway.
- **Changed:** the duplicate supervisor-side signature check goes. The
  restart runbook's step 9 moves to the application ("the application
  verifies the authorization and runs the fixed release"; a refusal still
  stops startup with exit class `release`). A5's sentence is reworded.
- **Work:** the smallest: `authorize()` in production builds the authority
  from pinned digests without the helper. Tests and the two documents.
- **Risk:** a compromised application binary could skip verification, but
  such a binary already controls block signing, so the supervisor's check
  adds little there. The executables still come only from root-owned,
  digest-pinned files installed from a digest-checked bundle.

## Recommendation

**D**, with B as the fallback if P01 wants the supervisor to keep a
signature-derived check. D keeps the reviewed E02 role model and removes a
duplicated check rather than adding a new privileged path. A and C add
security-relevant machinery to fix an ordering problem.

## After the decision

- Implement it in its own PR, with a test that starts the production
  supervisor path (the test that would have caught this).
- Update A5, the restart runbook and, for A or C, the E02 record.
- Rerun H4 on #12. Its other two findings (ufw detection and the 32 MiB
  helper bound) are already fixed there.
