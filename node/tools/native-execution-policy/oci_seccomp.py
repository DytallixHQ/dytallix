#!/usr/bin/env python3
"""Express the rendered unit syscall policy as an OCI seccomp profile.

Diagnostic only. It lets container runs exercise the same deny list that
`render.py` emits as systemd `SystemCallFilter`, so filter incompatibilities
surface before a native service run. It does not reproduce every systemd
control (for example `SystemCallArchitectures` or `MemoryDenyWriteExecute`
unless `--memory-deny-write-execute` is given) and qualifies nothing.
"""

import argparse
import json
from pathlib import Path

from render import NO_SOCKET_DENY, SYSTEM_CALL_DENY

# systemd's syscall groups used by the deny expression (src/shared/seccomp-util.c).
SYSTEMD_GROUPS = {
    "@mount": ["chroot", "fsconfig", "fsmount", "fsopen", "fspick", "mount", "mount_setattr",
               "move_mount", "open_tree", "pivot_root", "umount", "umount2"],
}
ARCHITECTURES = ["SCMP_ARCH_X86_64", "SCMP_ARCH_AARCH64"]
EPERM = 1
PROT_WRITE, PROT_EXEC, SHM_EXEC = 0x2, 0x4, 0o100000


def deny_list(expression):
    """Syscall names denied by a systemd `~name ...` deny expression."""
    if not expression.startswith("~"):
        raise ValueError("expected a systemd deny expression starting with ~")
    names = set()
    for item in expression[1:].split():
        if item.startswith("@"):
            if item not in SYSTEMD_GROUPS:
                raise ValueError(f"unknown systemd syscall group {item}")
            names.update(SYSTEMD_GROUPS[item])
        else:
            names.add(item)
    return sorted(names)


def memory_deny_write_execute():
    """systemd MemoryDenyWriteExecute: no W+X mappings, no later PROT_EXEC."""
    def rule(name, mask):
        return {"names": [name], "action": "SCMP_ACT_ERRNO", "errnoRet": EPERM,
                "args": [{"index": 2, "value": mask, "valueTwo": mask, "op": "SCMP_CMP_MASKED_EQ"}]}
    return [rule("mmap", PROT_WRITE | PROT_EXEC), rule("mprotect", PROT_EXEC),
            rule("pkey_mprotect", PROT_EXEC), rule("shmat", SHM_EXEC)]


def oci_profile(sockets=True, mdwe=False):
    expression = SYSTEM_CALL_DENY + ("" if sockets else " " + NO_SOCKET_DENY)
    syscalls = [{"names": deny_list(expression), "action": "SCMP_ACT_ERRNO", "errnoRet": EPERM}]
    if mdwe:
        syscalls += memory_deny_write_execute()
    return {"defaultAction": "SCMP_ACT_ALLOW", "architectures": ARCHITECTURES, "syscalls": syscalls}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--no-sockets", action="store_true", help="unit declares no network sockets")
    parser.add_argument("--memory-deny-write-execute", action="store_true")
    args = parser.parse_args()
    profile = oci_profile(sockets=not args.no_sockets, mdwe=args.memory_deny_write_execute)
    args.output.write_text(json.dumps(profile, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
