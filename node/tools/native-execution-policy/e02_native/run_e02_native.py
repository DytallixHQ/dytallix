#!/usr/bin/env python3
"""E02 native checks for a disposable Linux host with AppArmor (a CI runner).

Renders the maintained four-role policy with `production_roles.py`, loads it
in enforce mode and checks, with NoNewPrivileges:

1. Startup admission (`require_enforced_profiles`, the supervisor's check)
   against the live kernel: as root through the profile inventory, and as the
   non-root service UID under the rendered supervisor profile through its own
   label. Refused when unconfined, in complain mode, or (root) when a profile
   is absent; an absent helper profile also makes the owner's exec fail.
2. The full STOP/CONT matrix across two units: 6 allowed pairs, 58 denied.
3. The real Go root verifier: a direct supervisor launch is refused before
   READY; an owned launch verifies a valid request, fails closed on a changed
   one, and exits when its owner dies.
4. The production syscall filter (systemd `SystemCallFilter`,
   `MemoryDenyWriteExecute`, `SystemCallArchitectures=native`) on the owned
   helper request under AppArmor and on the Rust retained-pidfd pause fixture.

Diagnostic only: synthetic C parents stand in for the Rust supervisor and
application; T03 runs the frozen service candidate. Run as root on a host
that can be discarded. Every loaded profile and installed file is removed.
"""

import argparse
import hashlib
import json
import os
import platform
import re
import select
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
POLICY_TOOLS = HERE.parent
ROOT = Path("/opt/dyt-e02-ci")  # compiled into the C probes as E02_ROOT
UID = GID = 41001
LIVE = Path("/sys/kernel/security/apparmor/profiles")
REMOVE = Path("/sys/kernel/security/apparmor/.remove")
ROLES = ("supervisor", "application-owner", "workload", "helper")
PROBE = {"supervisor": "supervisor", "application-owner": "app", "workload": "bridge", "helper": "root_helper"}
NAMES = ("supervisor", "app", "bridge", "engine", "adapter", "root_helper")
ROLE_MAP = {"service_supervisor": "supervisor", "consensus_stdio": "app", "consensus_bridge": "bridge",
            "consensus_engine": "engine", "http_adapter": "adapter",
            "genesis_bootstrap_verifier": "root_helper", "control_verifier": "root_helper"}

sys.path.insert(0, str(POLICY_TOOLS))
import production_roles  # noqa: E402
import render  # noqa: E402
from test_render import fixture  # noqa: E402

LOG = []
CHECKS = []


def log(line):
    LOG.append(line)
    print(line, flush=True)


def check(name, ok, detail=""):
    CHECKS.append({"check": name, "status": "PASS" if ok else "FAIL", "detail": detail})
    log(f"CHECK {name} {'PASS' if ok else 'FAIL'} {detail}".rstrip())
    if not ok:
        raise RuntimeError(f"{name} failed: {detail}")


def apparmor_denials():
    """Recent kernel AppArmor denials, for a failed check's detail."""
    r = subprocess.run(["journalctl", "-k", "-o", "cat", "--since", "-5 min"],
                       capture_output=True, text=True, timeout=30)
    lines = [l for l in r.stdout.splitlines() if 'apparmor="DENIED"' in l]
    return " || ".join(lines[-8:])


def run(*args, check_exit=True, timeout=60):
    result = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    if check_exit and result.returncode != 0:
        raise RuntimeError(f"{args[0]} exit {result.returncode}: {result.stderr.strip()}")
    return result


def digest(path):
    raw = Path(path).read_bytes()
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(),
            "sha512": hashlib.sha512(raw).hexdigest()}


# ---------------------------------------------------------------- policy ---

class Policy:
    """One rendered, loaded policy for a set of installed role binaries."""

    def __init__(self, tag, units, readonly=()):
        self.tag = tag
        self.directory = ROOT / f"policy-{tag}"
        self.directory.mkdir()
        catalog_json, mapping, request = fixture()
        catalog = json.loads(catalog_json)
        hashes = {name: digest(ROOT / "bin" / name) for name in NAMES}
        catalog["members"] = [{"id": n, "kind": "executable", **hashes[n]} for n in NAMES]
        catalog["roles"] = [{"role": r, "member_id": ROLE_MAP[r], "runtime_profile_id": "all"}
                            for r in sorted(ROLE_MAP)]
        catalog["runtime_profiles"] = [{"id": "all", "member_ids": [],
                                        "mapping_policy": "linux-observed-code-v1",
                                        "interpreted_member_ids": []}]
        catalog_bytes = json.dumps(catalog, sort_keys=True, separators=(",", ":")).encode()
        mapping["members"] = [{"id": n, "path": str(ROOT / "bin" / n)} for n in NAMES]
        request["catalog_sha512"] = hashlib.sha512(catalog_bytes).hexdigest()
        request["service_uids"] = [UID]
        request["writable_roots"] = ["/var/lib/dyt-e02-ci/state", "/run/dyt-e02-ci/runtime"]
        request["readonly_files"] = [str(ROOT / "config.json")] + [str(ROOT / r) for r in readonly]
        request["network"]["namespace_path"] = "/run/netns/dyt-e02-ci"
        request["network"]["launch_channel"] = render.LAUNCH_CHANNEL
        files = production_roles.generate(catalog_bytes, mapping, request,
                                          [{"unit": u, "uid": UID, "gid": GID} for u in units],
                                          candidate_only=True)
        for name, content in files.items():
            (self.directory / name).write_bytes(content)
        self.profile = self.directory / "apparmor.profile"
        self.matrix = json.loads(files["role-matrix.json"])
        self.units = {row["unit"]: row for row in self.matrix["units"]}
        self.prefix = "dyt-role-" + self.matrix["identity_sha256"][:20]
        self.loaded = False
        log(f"policy {tag} identity={self.matrix['identity_sha256']} "
            f"profile_sha256={hashlib.sha256(files['apparmor.profile']).hexdigest()} "
            f"binaries={json.dumps({n: hashes[n]['sha256'] for n in NAMES}, sort_keys=True)}")

    def label(self, unit, role):
        return self.units[unit][role + "_label"]

    def required(self, unit):
        return [self.label(unit, r) for r in ROLES]

    def load(self, complain=False):
        run("apparmor_parser", "-Q", "-T", "-K", str(self.profile))
        mode = ["-C"] if complain else []
        run("apparmor_parser", "-r" if self.loaded else "-a", *mode, "-T", "-K", str(self.profile))
        self.loaded = True

    def remove_profile(self, name):
        REMOVE.write_text(name)

    def unload(self):
        if self.prefix in LIVE.read_text():
            run("apparmor_parser", "-R", "-K", str(self.profile), check_exit=False)
            for line in LIVE.read_text().splitlines():
                name = line.rsplit(" (", 1)[0]
                if name.startswith(self.prefix):
                    REMOVE.write_text(name)
        self.loaded = False
        return self.prefix not in LIVE.read_text()


def as_service(label, *command):
    """Run as the service UID under `label`. The probes set NoNewPrivileges."""
    return ["setpriv", f"--reuid={UID}", f"--regid={GID}", "--clear-groups",
            "--", "aa-exec", "-p", label, "--", *command]


def systemd_unit(*command, apparmor=None):
    """Run under the production unit syscall policy as the service UID."""
    properties = [f"User={UID}", f"Group={GID}", "NoNewPrivileges=yes",
                  f"SystemCallFilter={render.SYSTEM_CALL_DENY}", "SystemCallErrorNumber=EPERM",
                  "SystemCallArchitectures=native", "MemoryDenyWriteExecute=yes"]
    if apparmor:
        properties.append(f"AppArmorProfile={apparmor}")
    args = ["systemd-run", "--quiet", "--wait", "--pipe", "--collect"]
    for p in properties:
        args += ["-p", p]
    return args + ["--", *command]


def kill_group(process):
    for signo in (signal.SIGCONT, signal.SIGKILL):
        try:
            os.killpg(process.pid, signo)
        except ProcessLookupError:
            pass
    try:
        process.communicate(timeout=2)
    except subprocess.TimeoutExpired:
        process.kill()
        process.communicate(timeout=2)


def process_state(pid):
    status = Path(f"/proc/{pid}/status").read_text()
    values = dict(re.findall(r"^(Uid|Gid|NoNewPrivs|Seccomp):\s*([^\n]+)", status, re.M))
    return {"exe": os.readlink(f"/proc/{pid}/exe"),
            "label": Path(f"/proc/{pid}/attr/current").read_text().strip(),
            "uid": values["Uid"].split(), "gid": values["Gid"].split(),
            "nnp": values["NoNewPrivs"].strip(), "seccomp": values["Seccomp"].strip()}


# ---------------------------------------------------------------- phases ---

def install(binaries):
    if (ROOT / "bin").exists():
        shutil.rmtree(ROOT / "bin")
    (ROOT / "bin").mkdir(mode=0o755)
    for name, source in binaries.items():
        target = ROOT / "bin" / name
        shutil.copyfile(source, target)
        os.chmod(target, 0o755)


def build_probes(cc, out):
    out.mkdir(parents=True, exist_ok=True)
    built = {}
    for source, ids in (("role_probe", range(1, 7)), ("helper_parent", (1, 2))):
        for role_id in ids:
            target = out / f"{source}-{role_id}"
            run(cc, "-static", "-O2", "-Wall", "-Wextra", "-Werror", f'-DE02_ROOT="{ROOT}"',
                f"-DROLE_ID={role_id}", "-o", str(target), str(HERE / f"{source}.c"))
            built[(source, role_id)] = target
    return built


def admission(policy):
    """Startup admission against the live kernel (root and service-UID paths)."""
    unit = "probe0"
    labels = policy.required(unit)
    supervisor = labels[0]
    binary = str(ROOT / "bin/supervisor")  # the static admission fixture

    def root_check():
        return run(binary, *labels, check_exit=False)

    def service_check(label=supervisor):
        return run(*as_service(label, binary, *labels), check_exit=False)

    def service_unconfined():
        return run("setpriv", f"--reuid={UID}", f"--regid={GID}", "--clear-groups",
                   "--", binary, *labels, check_exit=False)

    policy.load()
    r = root_check()
    check("admission.root.enforced", r.returncode == 0 and "ADMITTED" in r.stdout, r.stdout.strip())
    r = service_check()
    check("admission.service_uid.enforced", r.returncode == 0 and "ADMITTED" in r.stdout,
          (r.stdout + r.stderr).strip())
    r = service_unconfined()
    check("admission.service_uid.unconfined_refused", r.returncode == 1 and "REFUSED" in r.stdout,
          r.stdout.strip())
    policy.load(complain=True)
    r = root_check()
    check("admission.root.complain_refused", r.returncode == 1 and "REFUSED" in r.stdout, r.stdout.strip())
    r = service_check()
    check("admission.service_uid.complain_refused", r.returncode != 0 and "ADMITTED" not in r.stdout,
          (r.stdout + r.stderr).strip())
    policy.load()
    policy.remove_profile(policy.prefix + f"-{unit}-helper")
    r = root_check()
    check("admission.root.absent_helper_refused", r.returncode == 1 and "REFUSED" in r.stdout,
          r.stdout.strip())
    policy.load()
    r = root_check()
    check("admission.root.restored", r.returncode == 0 and "ADMITTED" in r.stdout, r.stdout.strip())


def victim(policy, unit, role):
    command = as_service(policy.label(unit, "supervisor"), str(ROOT / "bin/supervisor"),
                         "role", PROBE[role], "wait")
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                               start_new_session=True)
    data = bytearray()
    end = time.monotonic() + 5
    while time.monotonic() < end and b"\nREADY " not in data:
        ready, _, _ = select.select([process.stdout], [], [], max(0.0, end - time.monotonic()))
        if ready:
            part = os.read(process.stdout.fileno(), 4096)
            if not part:
                break
            data.extend(part)
    if b"\nREADY " not in data:
        kill_group(process)
        raise RuntimeError(f"victim {unit}:{role} not READY: {bytes(data)!r}")
    return process


def expect_process(process, policy, unit, role, mode="enforce"):
    state = process_state(process.pid)
    ok = (state["exe"] == str(ROOT / "bin" / PROBE[role])
          and state["label"] == f"{policy.label(unit, role)} ({mode})"
          and state["uid"] == [str(UID)] * 4 and state["gid"] == [str(GID)] * 4
          and state["nnp"] == "1" and state["seccomp"] == "2")
    if not ok:
        raise RuntimeError(f"unexpected process state for {unit}:{role}: {state}")


def signal_matrix(policy):
    policy.load()
    allowed = count = 0
    for source_unit in ("probe0", "probe1"):
        for target_unit in ("probe0", "probe1"):
            for source_role in ROLES:
                for target_role in ROLES:
                    target = victim(policy, target_unit, target_role)
                    try:
                        expect_process(target, policy, target_unit, target_role)
                        tester = run(*as_service(policy.label(source_unit, "supervisor"),
                                                 str(ROOT / "bin/supervisor"), "role",
                                                 PROBE[source_role], "check", str(target.pid)),
                                     check_exit=False, timeout=10)
                        out = tester.stdout
                        match = re.search(r"CHECK self=(\d+) target=(\d+) zero=(-?\d+):(\d+) "
                                          r"stop=(-?\d+):(\d+) cont=(-?\d+):(\d+)", out)
                        if tester.returncode != 0 or not match or int(match[2]) != target.pid or \
                                f"label={policy.label(source_unit, source_role)} (enforce)" not in out:
                            raise RuntimeError(f"tester failed: {out!r}")
                        stop, stop_errno, cont, cont_errno = map(int, match.group(5, 6, 7, 8))
                        expect = source_unit == target_unit and (
                            (source_role == "supervisor" and target_role in ("application-owner", "workload"))
                            or (source_role == "application-owner" and target_role == "helper"))
                        if expect:
                            ok = (stop, stop_errno, cont, cont_errno) == (0, 0, 0, 0)
                        else:
                            ok = stop == cont == -1 and stop_errno in (1, 13) and cont_errno in (1, 13)
                        log(f"CASE {source_unit}:{source_role} -> {target_unit}:{target_role} "
                            f"expected={'ALLOW' if expect else 'DENY'} stop={stop}:{stop_errno} "
                            f"cont={cont}:{cont_errno} {'ok' if ok else 'MISMATCH'}")
                        if not ok:
                            raise RuntimeError("signal matrix mismatch")
                        allowed += expect
                        count += 1
                    finally:
                        kill_group(target)
    check("signals.matrix", count == 64 and allowed == 6, f"cases={count} allowed={allowed} denied={count - allowed}")

    # A complain-mode stack is reported as such, never as enforce.
    policy.load(complain=True)
    process = victim(policy, "probe0", "application-owner")
    try:
        expect_process(process, policy, "probe0", "application-owner", mode="complain")
        check("roles.complain_label_not_enforce", True, process_state(process.pid)["label"])
    finally:
        kill_group(process)
    # An absent helper profile makes the owner's exec to the helper fail.
    policy.load()
    policy.remove_profile(policy.prefix + "-probe0-helper")
    r = run(*as_service(policy.label("probe0", "supervisor"), str(ROOT / "bin/supervisor"),
                        "role", "root_helper", "check", "1"), check_exit=False, timeout=10)
    check("roles.absent_helper_exec_refused", r.returncode == 70, f"exit={r.returncode}")
    policy.load()


def helper_cases(policy, helper_sha512):
    policy.load()
    supervisor = policy.label("probe0", "supervisor")
    binary = str(ROOT / "bin/supervisor")
    r = run(*as_service(supervisor, binary, "direct", helper_sha512), check_exit=False, timeout=15)
    check("helper.direct_launch_refused", r.returncode == 0 and "direct_ready=0" in r.stdout, r.stdout.strip())
    for mode, status, code in (("good", 0, 0), ("bad", 2, 2)):
        r = run(*as_service(supervisor, binary, mode, helper_sha512), check_exit=False, timeout=15)
        ok = (r.returncode == 0 and "guard_ready=1" in r.stdout and "protocol_ready=1" in r.stdout
              and f"result_status={status}" in r.stdout and f"helper_exit={code} " in r.stdout)
        check(f"helper.owned_{mode}_request", ok, r.stdout.strip().replace("\n", " | "))
    process = subprocess.Popen(as_service(supervisor, binary, "owner-death", helper_sha512),
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                               start_new_session=True)
    try:
        out = ""
        end = time.monotonic() + 8
        while time.monotonic() < end and "helper_pid=" not in out:
            ready, _, _ = select.select([process.stdout], [], [], 0.2)
            if ready:
                line = process.stdout.readline()
                if not line:
                    break
                out += line
        match = re.search(r"helper_pid=(\d+)", out)
        exited = False
        if match:
            pid = int(match[1])
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                path = Path(f"/proc/{pid}/stat")
                if not path.exists() or path.read_text().split(") ", 1)[1].startswith("Z"):
                    exited = True
                    break
                time.sleep(0.05)
        check("helper.exits_on_owner_death", bool(match) and exited, out.strip().replace("\n", " | "))
    finally:
        kill_group(process)
    for mode, status in (("good", 0), ("bad", 2)):
        r = run(*systemd_unit(binary, mode, helper_sha512, apparmor=supervisor), check_exit=False, timeout=30)
        ok = r.returncode == 0 and f"result_status={status}" in r.stdout
        detail = (r.stdout + r.stderr).strip().replace("\n", " | ")
        if not ok:
            detail = f"exit={r.returncode} {detail} denials: {apparmor_denials()}"
        check(f"filter.owned_{mode}_request_under_apparmor", ok, detail)


def filter_checks(pause_bin):
    target = ROOT / "bin/pause-fixture"
    shutil.copyfile(pause_bin, target)
    os.chmod(target, 0o755)
    r = run(*systemd_unit("/usr/bin/grep", "-E", "^(NoNewPrivs|Seccomp):", "/proc/self/status"),
            check_exit=False)
    check("filter.active", r.returncode == 0 and re.search(r"NoNewPrivs:\s*1", r.stdout) is not None
          and re.search(r"Seccomp:\s*2", r.stdout) is not None, r.stdout.strip().replace("\n", " "))
    for mode in ("paused", "unpaused"):
        r = run(*systemd_unit(str(target), "--samples", "3", "--total-millis", "3000",
                              "--cleanup-reserve-millis", "500", "--mode", mode),
                check_exit=False, timeout=90)
        line = next((l for l in r.stdout.splitlines() if l.startswith("{")), "{}")
        report = json.loads(line)
        ok = r.returncode == 0 and report.get("status") == "PASS_QUALIFICATION_ONLY"
        timings = [s.get("timing") for s in report.get("samples", [])]
        check(f"filter.rust_pause_{mode}", ok, json.dumps(timings)[:400] if ok else (r.stdout + r.stderr)[-600:])


# ------------------------------------------------------------------ main ---

def preflight():
    if os.geteuid() != 0 or platform.system() != "Linux":
        raise SystemExit("run as root on Linux")
    if Path("/sys/module/apparmor/parameters/enabled").read_text().strip() != "Y":
        raise SystemExit("AppArmor is not enabled")
    for tool in ("apparmor_parser", "aa-exec", "setpriv", "systemd-run"):
        if shutil.which(tool) is None:
            raise SystemExit(f"missing {tool}")
    if ROOT.exists():
        raise SystemExit(f"{ROOT} already exists")
    log(f"kernel={platform.release()} parser={run('apparmor_parser', '--version').stdout.splitlines()[0]}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go-helper", type=Path, required=True, help="static dytallix-root-verify")
    parser.add_argument("--fixture-dir", type=Path, required=True, help="policy.json, request.json, bad-request.json")
    parser.add_argument("--admission-bin", type=Path, required=True, help="static release-runtime-profile-admission")
    parser.add_argument("--pause-bin", type=Path, required=True, help="release-runtime-pause-qualification")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--cc", default="cc")
    args = parser.parse_args()
    preflight()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    policies = []
    failure = None
    try:
        ROOT.mkdir(mode=0o755)
        (ROOT / "config.json").write_text("{}\n")
        for name in ("policy.json", "request.json", "bad-request.json"):
            shutil.copyfile(args.fixture_dir / name, ROOT / name)
            os.chmod(ROOT / name, 0o644)
        probes = build_probes(args.cc, args.output_dir / "probes")
        role = {n: probes[("role_probe", i + 1)] for i, n in enumerate(NAMES)}

        install({**role, "supervisor": args.admission_bin})
        policies.append(Policy("admission", ["probe0"]))
        admission(policies[-1])
        check("cleanup.admission", policies[-1].unload())

        install(role)
        policies.append(Policy("signals", ["probe0", "probe1"]))
        signal_matrix(policies[-1])
        check("cleanup.signals", policies[-1].unload())

        install({**role, "supervisor": probes[("helper_parent", 1)], "app": probes[("helper_parent", 2)],
                 "root_helper": args.go_helper})
        policies.append(Policy("helper", ["probe0"],
                               readonly=("policy.json", "request.json", "bad-request.json")))
        helper_cases(policies[-1], digest(ROOT / "bin/root_helper")["sha512"])
        check("cleanup.helper", policies[-1].unload())

        filter_checks(args.pause_bin)
    except Exception as error:  # record, clean up, then fail
        failure = error
        log(f"FAILURE {type(error).__name__}: {error}")
    finally:
        for policy in policies:
            policy.unload()
        leftovers = [line for line in LIVE.read_text().splitlines()
                     if any(line.startswith(p.prefix) for p in policies)]
        shutil.rmtree(ROOT, ignore_errors=True)
        log(f"profile_cleanup={'PASS' if not leftovers else 'FAIL ' + repr(leftovers)}")
        summary = {"schema": "dytallix-e02-native-diagnostic-v1", "scope": "diagnostic; not T03 qualification",
                   "kernel": platform.release(), "status": "PASS" if failure is None and not leftovers else "FAIL",
                   "checks": CHECKS}
        (args.output_dir / "E02_NATIVE_SUMMARY.json").write_text(json.dumps(summary, indent=2) + "\n")
        (args.output_dir / "e02_native_raw.txt").write_text("\n".join(LOG) + "\n")
    if failure is not None or leftovers:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
