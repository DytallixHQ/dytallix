# Production start path audit (H4, 7 October 2026)

Task **E05**, host setup. This is engineering evidence, not T-suite or launch evidence.

## Why H4 found one defect per run

H4 (`.github/workflows/host-install.yml`) is the first end-to-end run of the
production start path on a real host:
- the real installer;
- the real systemd unit and AppArmor profiles;
- the production binaries;
- a non-root service account.

Every earlier test replaced at least one of these:
- the process tests used the test-only snapshot verifier;
- E02's native checks used root harnesses, synthetic parents and C probes;
- the unit tests used fake hosts.

The node stops at the first refusal and, by design, reports nothing for a
refusal before startup. So each run revealed only the next defect, and each
run took twelve minutes.

This audit walked the whole start path once instead:
1. the supervisor after its preflight;
2. the application from exec to `info`;
3. the engine, bridge and root helper;
4. the four AppArmor roles against what each process does.

Each check was compared with what the installer, the unit and the profiles
provide. Findings were then checked against H4's kernel traces wherever a run
had reached them.

## Findings

| # | Finding | Status |
| --- | --- | --- |
| 1 | `ufw` was detected through its oneshot unit, which stays active after `ufw disable` | Fixed (#12) |
| 2 | Root helper observation bound of 32 MiB, against a helper bound of 16 MiB | Fixed (#12, P01 7 October) |
| 3 | The supervisor's preflight ran the root helper, which E02 allows only for the application | Fixed (#14, option D) |
| 4 | Stacked labels lacked ptrace `readby`/`read`, so parents could not open their children's `/proc/<pid>/exe` | Fixed (#15, 1907fd5) |
| 5 | The runner's `/opt` is 0777. The node requires every ancestor of the root helper to be root-owned without group or other write. | Preflight check added (#15). The CI runner is set to 0755. Stock Ubuntu ships 0755. |
| 6 | **The engine read the published binding (`/etc/dytallix/<release>/binding.json`, root 0444) with the private-file reader, which requires 0400/0600 owned by the engine.** Every production engine would have stopped before creating its sockets. | Fixed (#15): the engine reads it as a published file. |
| 7 | Hosts with `state_sync` set: `/var/lib/dytallix/light-blocks` is in no profile, and the renderer's `readonly_directories` grants only the directory itself. The supervisor's state-sync check and the engine's reads would be refused. | Fixed (follow-up PR 2): the renderer's `readonly_trees` (read beneath, no write, map or execution), which `host_files.py` requests for a host whose plan sets `state_sync`. Not yet run on a host: H4 installs no syncing host. |
| 8 | Go's runtime signals its own threads (SIGURG, asynchronous preemption). The S component (and A, for the helper) has no signal rule for the process's own stacked label, so this is denied. Go ignores the error, but loses asynchronous preemption and logs one denial per thread. | Fixed (follow-up PR): a narrow `urg` rule for each stack's own label; a test checks that self-signals pass and that no two stacks can exchange `urg`. |
| 9 | Fallback reads that the profiles deny and log: Go's cgroup and `/sys` reads, `/proc/sys/net/core/somaxconn`, `/etc/localtime`, RocksDB's thread-name writes to `/proc/self/task/*/comm`, glibc `get_nprocs`. | Noise only. Each has a fallback. Decide whether to grant them or accept the denials. |
| 10 | A SIGKILL leaves Unix sockets behind (`abci/app.sock`, `data/rpc*.sock`). The next start refuses them: the supervisor's state walk and the bridge's existing-path checks. | Fixed (follow-up PR): with both lifecycle leases held (children inherit them, so none can still run), the supervisor removes the three endpoint paths if each is a socket owned by the service user without group or other bits. Anything else there is still refused. |
| 11 | Sentry snapshots: RocksDB checkpoints hard-link from `appdb` into the snapshot directory, and the profiles grant no `l`. | Unclear. Separate mounts give EXDEV, and RocksDB then copies. Check on the first sentry. |
| 12 | At the bridge's exec into `S//&W`, AppArmor rechecks the inherited seqpacket launch channel against each profile of the stack, using the other components as bare peer labels. The S profile named the bare A and H components but not W, so the channel was revoked ("failed peer label match") and the bridge exited. The supervisor reported `guard_ready` and ECONNRESET. The profile audit had wrongly called this covered; H4 found it. | Fixed (#15): S names the bare W, and a test requires every stack component to name the others. |
| — | AppArmor `getattr` on the helper's ancestor directories and on `/proc/<pid>/` (proposed by two audits) | Refuted by H4's trace: the ancestor `statx` calls succeeded and no denial was logged. |

## What E02's native checks never exercise

Taken from `tools/native-execution-policy/e02_native/run_e02_native.py`:
- a parent reading a child's `/proc` (findings 4 and 6 would have surfaced here);
- the controlled pause under AppArmor;
- TERM and KILL through the process group, and parent-death from S;
- the Rust S→A and S→W ownership handshake;
- real Go and Rust workloads under `S//&W` and `A//&S` (finding 8);
- Unix stream sockets (ABCI, RPC);
- I/O in the writable roots (locks, RocksDB, metrics, snapshots, light blocks).

H4 now covers the validator's path through all of these. The sentry and
endpoint paths (snapshots, adapter, channel) still have no end-to-end run.

## Changes to how H4 runs

- **Cached release build.** The release set is cached by the git blobs of
  `node/`, `sdk/` and `release/`, excluding `node/tools` and `node/docs`, which
  no release build reads. A change to the host tools alone reruns in minutes.
- **Failure diagnostics** (`staging_host.py`):
  - the loaded unit;
  - AppArmor records from `audit.log`;
  - a kernel trace (bpftrace) of the supervisor and the application under the
    full unit;
  - an unconfined strace run;
  - the root helper's ancestor chain.

## Proposed next

1. Merge #15, which contains findings 4–6. Rerun H4 for the validator's first
   real start, then its stop and restart.
2. Decide findings 7, 8 and 10.
3. Decide whether production binaries should report a fixed, non-secret
   refusal stage before startup. The supervisor already reports one after
   startup begins. Most of H4's rounds would have taken one run. This changes
   the reviewed "no output" design.
4. Add H4 runs for a sentry and an endpoint.
