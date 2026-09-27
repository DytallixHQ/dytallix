# E02 helper ownership and role confinement

Status: **E02 source engineering complete; the direct-exec design is
accepted. Native qualification is T03.** 26 September 2026. This record supersedes the
25 September E02 records in the launch decision register for the maintained
source (`mainnet/node`). It grants no launch approval. Mainnet remains
`NO_GO`.

## Requirement

Replace Candidate16's delegated-cgroup helper control, define four directed
roles per service unit with enforceable AppArmor profiles, and let the
application own the static `root_helper` through a retained pidfd, keeping the
helper deadline, one exact-map observation, explicit continuation and fatal
cleanup on uncertain state (engineering split, E02.1–E02.5).

## Maintained implementation

| Subtask | Source |
| --- | --- |
| E02.1 Retire delegated-cgroup control | No maintained source or unit writes cgroup control files or enables systemd delegation. `scripts/check_helper_control.py` enforces this in CI. Candidate16 (`BLOCKED_UNSAFE`: the workload could move the helper into the delegated parent) is not in the tree |
| E02.2 Four directed roles | `tools/native-execution-policy/production_roles.py` renders supervisor (S), application owner (A), workload (W) and helper (H) profiles per unit as additive stacks: S, A//&S, S//&W, A//&H//&S. Execution: S→A, S→W, A→H. STOP/CONT: S→A, S→W, A→H only; W and H have explicit send denials; no cross-unit pair |
| E02.3 Enforced policy and startup refusal | The renderer keeps the base file, mapping, syscall, namespace and `NoNewPrivileges` controls (`render.py`). `native-supervisor` rejects the old two-role admission and runs the startup profile check before preparation and again before startup (see the defect below: root reads the kernel inventory; the service UID must run under its exact enforced supervisor label). Each child's exact enforced label is checked against its role before it is released (`crates/native-supervisor/src/{config,processes}.rs`, `crates/release-runtime/src/ownership_security.rs`) |
| E02.4 Retained-pidfd static helper | The application is the single `Child` owner. `observation::static_helper` verifies the helper file (`VerifiedStaticHelperFile`) and captures the owned child (`OwnedStaticHelperIdentity`); `observation::controlled_pause` sends STOP through the retained pidfd, waits for the stop, performs one exact-map observation, sends CONT and confirms continuation. Emergency, upgrade and handover verification reach it through `root_genesis::run_verified_helper` → `observed_execution::run` (Linux only; other builds refuse) |
| E02.5 Deadline, observation, cleanup, binding | One total helper deadline with a cleanup reserve; a failed or uncertain step takes the fatal cleanup path. The helper's path, bytes, SHA-256, SHA-512 and role labels are checked against the verified service candidate. The Go helper (`consensus/owner-guard/guard_linux.go`) requires the exact A//&H//&S label before READY or any request |

## Defect fixed: startup check could not run as the service UID

`require_enforced_profiles` read `/sys/kernel/security/apparmor/profiles` to
confirm that all four profiles are loaded in enforce mode. The kernel opens
that file only for root (Linux 6.8 `profiles_open` →
`aa_current_policy_view_capable`: euid or egid 0, despite mode 0444), and the
rendered supervisor profile grants no read of it. The supervisor runs as its
unit's non-root UID, so every production startup would have been refused. The
earlier native runs used root harnesses and synthetic parents, so they never
reached this path.

The check now reads the inventory only as root (for example a privileged
pre-start step). Otherwise it requires the caller to run under exactly the
supervisor profile in enforce mode, which proves that profile is loaded and
enforced. The other profiles are proven where they are used:

- a child is released (GO) only after its exact stacked label reads
  `(enforce)`, checked before and after its initial observation; the kernel
  reports a stack as enforce only when every component profile is (6.8
  `label_modename`), and complain or mixed labels fail the parser;
- the application checks the helper's `A//&H//&S (enforce)` label the same
  way, and the helper's own guard requires it before READY;
- the `Px` exec to an absent profile fails.

## Production syscall filter

The unit's filter is rendered as systemd `SystemCallFilter`
(`render.SYSTEM_CALL_DENY`): `~@mount memfd_create ptrace process_vm_writev
recvmsg recvmmsg pidfd_getfd io_uring_setup io_uring_enter
io_uring_register`, plus `socket socketpair` for units without network
sockets, with `SystemCallErrorNumber=EPERM`, `NoNewPrivileges`,
`MemoryDenyWriteExecute` and `SystemCallArchitectures=native`. All earlier
native probes replaced it with an allow-all filter.

Compatibility review of the maintained code (26 September):

- No maintained Rust or Go source, and none of the 84 upstream Comet packages
  in the PQC-only graphs, calls a denied syscall.
- The Go runtime's pidfd support uses `pidfd_open`, `waitid(P_PIDFD)` and
  `pidfd_send_signal` (allowed), not `pidfd_getfd`.
- RocksDB is built without its `io-uring` feature.

`tools/native-execution-policy/oci_seccomp.py` expresses the same deny list as
an OCI seccomp profile for container diagnostics (it imports the renderer's
constants). A container run with that profile and no-new-privileges returned
EPERM for `memfd_create`, `ptrace`, `mount` and `recvmsg`, where the baseline
container allowed `memfd_create` and `ptrace`. The Rust pause fixture could not
run in that container: Docker's amd64 emulation on the development Mac
returns ENOSYS for `pidfd_open` with or without the filter. The real path must
run natively.

## Native evidence to date (diagnostic)

Disposable runs on 25 September (Linux 6.8.0, AppArmor parser 4.0.1; raw logs
and hashes under `decision-register/core-function-alignment/e02-native-probe-20260925/`).
The latest records pin exactly the sources now in `mainnet/node`.

- The unstacked `Px` design was rejected by the kernel under
  `NoNewPrivileges`; the additive stacks were accepted (S→A, S→W, A→H).
- Full synthetic signal matrix across two same-UID units: 6 allowed pairs
  (S→A, S→W, A→H per unit), 58 denied.
- Real Go root verifier: direct S→H launch rejected by the label guard with
  no READY; S→A→H sent READY; a valid request returned a result, a changed
  signature returned status 2; the helper exited when its owner died.
- Cross-built Rust pause fixture: three paused and three unpaused owned-child
  runs completed and reaped (1.36–1.47 s paused envelopes) without signal
  isolation or the production filter.

All used synthetic parents and an allow-all seccomp filter.

## Decision: direct helper execution (accepted 26 September 2026, Rick Glenn)

The additive stack keeps S in A, so S must permit the helper executable. That
also lets S start H directly, producing the incomplete label H//&S. The
accepted design (`E02_DIRECT_HELPER_EXEC_DECISION_2026-09-25.md`) permits that
process creation because the helper rejects it before READY or any request,
on these conditions: keep the exact label guard in the production helper; bind helper
hashes, labels and policy identity to the candidate; re-prove direct
rejection and owned acceptance with the production Rust parent, filter and
profiles in T03. If rejected, E02 needs a different process design: the
unstacked `Px` and nested `Cx` designs fail on the selected kernel.

## Native diagnostic job

`.github/workflows/mainnet.yml` job `e02-native` runs
`tools/native-execution-policy/e02_native/run_e02_native.py` as root on a
disposable Ubuntu 24.04 runner (AppArmor enabled). It renders the maintained
policy, loads it in enforce mode and checks:

1. Startup admission against the live kernel with the real check: admitted as
   root and as the service UID under the supervisor profile; refused when
   unconfined, in complain mode, or (root) with the helper profile removed.
2. The 64-case STOP/CONT matrix across two units (6 allowed, 58 denied); a
   complain-mode stack reads `(complain)`, not `(enforce)`; the owner's exec to
   a removed helper profile fails.
3. The real Go root verifier: direct supervisor launch refused before READY;
   owned launch verifies a valid request and fails closed on a changed one;
   the helper exits when its owner dies.
4. Under the production unit syscall policy (`systemd-run` with the rendered
   `SystemCallFilter`, `MemoryDenyWriteExecute`, native architecture only):
   the owned helper request inside the AppArmor stack, and the Rust
   retained-pidfd pause fixture, paused and unpaused.

The public fixture (policy, valid and changed request) is generated at run
time by `TestExportHelperQualificationFixture`; the signing key stays in
memory. The job runs once the repository's GitHub Actions billing lock is
cleared. Its results are diagnostic, not T03 qualification.

## T03 handoff

T03 runs on one frozen Linux x86_64 service candidate with the exact binaries,
catalog, profiles, admission record and helper:

1. Startup refusal with an absent, complain-mode and correct profile set,
   under the production syscall filter.
2. Direct S→H rejection and S→A→H acceptance with the production Rust parent.
3. The complete STOP/CONT matrix on the actual service processes, including
   cross-unit and same-role pairs.
4. The full owner path: helper request and result through the Rust
   application, paused-helper timing within the helper deadline, owner death,
   unexpected continuation, deadline expiry and cleanup failure.
5. The 40 service checks, emergency checks and handover checks.

These need a Linux x86_64 host with AppArmor, root and enough disk for the
release build, running the frozen candidate; the `e02-native` job covers the
kernel-level behaviour beforehand on disposable runners.
