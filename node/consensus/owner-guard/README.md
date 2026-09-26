# Inherited owner startup guard

Linux executable entry points call `Run` once from `main`. The function locks that goroutine to its current OS thread. It never unlocks the thread. The callback must retain process ownership until its work and cleanup finish. Returning to `main` ends the process.

FD7 is an unnamed UNIX SOCK_SEQPACKET control channel. FD8 is the immediate parent's inherited pidfd. FD3/4 application pipes and FD5/6 lifecycle leases remain separate. The parent queues the complete bootstrap before spawn. The child reads it without blocking. Missing bootstrap is an error. The child does not reopen a PID supplied through an argument or environment variable.

The bootstrap contains 128 bytes. Integer fields use big endian encoding:

| Bytes | Field |
| --- | --- |
| 0–7 | DYTOWN01 |
| 8–11 | Role: application1, bridge2, engine3, adapter4, helper5 |
| 12–15 | Immediate owner PID |
| 16–19 | Creator TID, equal to owner PID |
| 20–23 | Effective UID |
| 24–31 | Absolute CLOCK_MONOTONIC deadline in nanoseconds |
| 32–39 | Parent pidfd device |
| 40–47 | Parent pidfd inode |
| 48–111 | Nonzero release or context SHA512 |
| 112–127 | Zero |

The child binds socket peer credentials, role, UID, owner PID, pidfd identity and live state. It requires NoNewPrivs1 and seccomp mode2. A helper also hashes its actual `/proc/self/exe` and requires a match with the context SHA512. Engine and bridge context authorization belongs to the parent catalog and artifact checks.

The child sets SIGKILL as its parent-death signal, reads it on the same OS thread, and checks the owner again. It returns the complete frame with DYTRDY01 as its magic. The parent must independently inspect the owned child before sending the exact DYTGO001 token. The child checks the owner and signal again before calling the executable callback.

The parent must bind the exact executable, security label, mount policy, release and unit before GO. The bootstrap alone does not supply those trusted policy values. Guard readiness is not full production admission.

The guard uses recvfrom and sendto. It cannot import ancillary descriptors. It retains FD8 with close-on-exec until `Run` returns. It closes FD7 after admission. Re-execution has no supported bootstrap reset or fallback. Unsupported operating systems fail closed.

An execution transition can clear the pre-exec parent-death signal. This guard restores it before work. It does not provide continuous kernel protection during that transition. The retained creator-thread contract and external startup deadlines remain required.
