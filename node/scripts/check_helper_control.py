#!/usr/bin/env python3
"""E02: no delegated-cgroup helper control in maintained sources.

Candidate16's design delegated a cgroup to the service and froze the helper
through a child cgroup. A native test showed the workload could move the
helper into the delegated parent (BLOCKED_UNSAFE). The helper is paused only
through the application's retained pidfd. This check fails if a maintained
source or unit writes cgroup control files or enables systemd delegation.
"""

import argparse
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SCANNED = (
    "crates",
    "dytallix-fast-launch/node/src",
    "consensus/owner-guard",
    "consensus/root-authorization",
    "consensus/cometbft/internal",
    "consensus/cometbft/cmd",
    "consensus/pqc-http-adapter/src",
    "tools/native-execution-policy",
    "deploy",
)
SUFFIXES = {".rs", ".go", ".py", ".sh", ".service", ".json", ".toml", ".conf", ".c"}
PATTERN = re.compile(
    r"cgroup\.(procs|threads|subtree_control|freeze|kill)\b"
    r"|\bDelegate(Subgroup)?\s*[=:]"
    r"|/sys/fs/cgroup"
    r"|\bCLONE_INTO_CGROUP\b"
)


def is_test(path):
    name = path.name
    return (name.endswith(("_test.go", "_tests.rs")) or name.startswith("test_")
            or "tests" in path.parts or "testdata" in path.parts)


def findings(root=ROOT):
    matches = []
    for base in SCANNED:
        for path in sorted((root / base).rglob("*")):
            if not path.is_file() or path.suffix not in SUFFIXES or is_test(path):
                continue
            if path.resolve() == Path(__file__).resolve():
                continue
            for number, line in enumerate(path.read_text(errors="replace").splitlines(), 1):
                if PATTERN.search(line):
                    matches.append(f"{path.relative_to(root)}:{number}: {line.strip()}")
    return matches


def main():
    argparse.ArgumentParser(description=__doc__).parse_args()
    matches = findings()
    for match in matches:
        print(match, file=sys.stderr)
    if matches:
        print("Delegated-cgroup helper control is prohibited (E02); pause helpers through the retained pidfd",
              file=sys.stderr)
        raise SystemExit(1)
    print("PASS: no cgroup control writes or systemd delegation in maintained sources")


if __name__ == "__main__":
    main()
