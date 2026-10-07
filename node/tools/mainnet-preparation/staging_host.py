#!/usr/bin/env python3
"""Install a staging bundle on this host and start the node (E05; host
setup v1, step H4: node/docs/architecture/host-setup-v1.md). CI runs it on
an Ubuntu 24.04 runner; it also rehearses a staging host by hand.

  staging_host.py prepare --release DIR --work DIR [--address IP]
  sudo staging_host.py install --work DIR [--label validator-1] [--height N] [--timeout S]
  sudo staging_host.py wipe --work DIR [--label validator-1]

prepare (as a normal user) writes a staging pin plan from the template
(chain dytallix-staging-1; this host's address for the validator, RFC 5737
documentation addresses for the others), runs the offline key step with the
release's tools and types each seal code back, and builds the staging
chain and every host's bundle (staging_chain.py). The seal codes go to
WORK/seal-codes.json (mode 0600): this chain is throwaway.

install (as root) unpacks the host's bundle and runs its install.sh,
typing the seal code, starts dytallix-node, waits until the application's
metrics report the height, runs the bundle's verify, then stops and starts
the node and waits for two more blocks: a lone validator must rejoin after
a restart. wipe runs the bundle's wipe.sh and checks that nothing is left.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import time

HERE = Path(__file__).resolve().parent
MAINNET = HERE.parents[2]
TEMPLATE = MAINNET / 'launch' / 'hosts' / 'PIN_PLAN.template.json'
CHAIN_ID = 'dytallix-staging-1'
UNIT = 'dytallix-node'
APP_METRICS = Path('/var/lib/dytallix/metrics/dytallix-app.prom')
HEIGHT = re.compile(r'^dytallix_app_height (\d+)$', re.M)


class Failed(Exception):
    pass


def say(message):
    print(message, flush=True)


def host_address():
    """The address this host routes from: on a runner, its interface address."""
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect(('192.0.2.1', 9))  # no packet is sent
        return probe.getsockname()[0]


def staging_plan(address):
    plan = json.loads(TEMPLATE.read_bytes())
    plan['chain_id'] = CHAIN_ID
    others = iter(f'192.0.2.{n}' for n in range(11, 30))
    for host in plan['hosts']:
        ip = address if host['role'] == 'validator' else next(others)
        for field in ('p2p', 'channel', 'status'):
            if host[field]:
                host[field] = ip + host[field][host[field].rindex(':'):]
    return plan


def prepare(release, work, address):
    release, work = Path(release), Path(work)
    work.mkdir(parents=True)
    tools = release / 'bin'
    plan = staging_plan(address)
    (work / 'plan.json').write_text(json.dumps(plan, indent=2) + '\n')
    say(f'== staging plan: {CHAIN_ID}, validator at {address}')
    # The key step, typing each printed code back as the founder would.
    process = subprocess.Popen([sys.executable, '-u', str(HERE / 'host_keys.py'), '--plan', str(work / 'plan.json'),
                                '--bin', str(tools), '--staging', str(work / 'staging'), '--out', str(work / 'keys')],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    codes, line = {}, None
    for out in process.stdout:
        print(out, end='', flush=True)
        for row in out.splitlines():
            if row.startswith('dytallix-seal-'):
                line = row
                codes[row.split()[0][len('dytallix-seal-'):]] = row
        if 'type it back from the paper' in out:
            process.stdin.write(line + '\n')
            process.stdin.flush()
    if process.wait() != 0:
        raise Failed('the key step failed')
    descriptor = os.open(work / 'seal-codes.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        json.dump(codes, file, indent=2)
    say('== staging chain')
    result = subprocess.run([sys.executable, '-B', str(HERE / 'staging_chain.py'), '--plan', str(work / 'keys' / 'PIN_PLAN.json'),
                             '--keys', str(work / 'keys'), '--release', str(release), '--tools', str(tools),
                             '--out', str(work / 'chain')])
    if result.returncode != 0:
        raise Failed('the staging chain build failed')
    return json.loads((work / 'chain' / 'STAGING.json').read_bytes())


def unpack(work, label):
    target = Path(work) / f'host-{label}'
    if not target.exists():
        with tarfile.open(Path(work) / 'chain' / 'bundles' / f'{label}.bundle.tar') as tar:
            tar.extractall(target, filter='tar')
    return target / 'dytallix-host'


def height():
    try:
        found = HEIGHT.search(APP_METRICS.read_text())
    except OSError:
        return None
    return int(found.group(1)) if found else None


def diagnostics():
    # The supervisor prints nothing when it refuses before startup, so also
    # show the unit as loaded and the kernel's AppArmor denials and seccomp
    # records.
    for args in (['systemctl', 'status', '--no-pager', f'{UNIT}.service'],
                 ['journalctl', '-u', f'{UNIT}.service', '--no-pager', '-n', '300', '-o', 'short-precise'],
                 ['systemctl', 'cat', '--no-pager', f'{UNIT}.service'],
                 # Every transport: with journald's audit socket, AppArmor
                 # denials arrive through audit and never reach the kernel log.
                 ['journalctl', '--no-pager', '-o', 'short-precise', '-n', '200',
                  '--grep', 'apparmor="DENIED"|type=1326|seccomp'],
                 ['aa-status'], ['nft', 'list', 'table', 'inet', 'dytallix_node']):
        say('$ ' + ' '.join(args))
        subprocess.run(args)
    traced_start()
    kernel_traced_start()


def traced_start():
    """The unit's ExecStart once more, as its account but outside systemd and
    its sandbox, under strace. The supervisor prints nothing when it refuses
    before startup; its last calls show which check refused, and getting
    further here than under the unit points at the sandbox."""
    unit = Path(f'/etc/systemd/system/{UNIT}.service')
    text = unit.read_text() if unit.exists() else ''
    command = re.search(r'^ExecStart=(.+)$', text, re.M)
    user = re.search(r'^User=(\d+)$', text, re.M)
    group = re.search(r'^Group=(\d+)$', text, re.M)
    if not (command and user and group and shutil.which('strace')):
        say('(no traced start: the unit or strace is missing)')
        return
    trace = Path('/tmp/dytallix-supervisor.strace')
    args = ['setpriv', f'--reuid={user.group(1)}', f'--regid={group.group(1)}', '--clear-groups',
            'strace', '-f', '-qq', '-s', '200', '-o', str(trace), '--', *command.group(1).split()]
    say('$ ' + ' '.join(args))
    process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    try:
        output, _ = process.communicate(timeout=30)
        say(f'exit {process.returncode}; output {output[-2000:]!r}')
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.communicate()
        say('still running after 30 s outside the sandbox (stopped)')
    lines = trace.read_text(errors='replace').splitlines() if trace.exists() else []
    say(f'-- last {min(len(lines), 150)} of {len(lines)} traced calls --')
    for line in lines[-150:]:
        say(line)


# Kernel tracepoints, so nothing in the unit's sandbox can block the trace
# (ptrace is denied there). Read contents are printed only for small reads
# after a /proc open: never for key or configuration files. BPF programs have
# a 512-byte stack, so strings stay at 96 bytes, each probe holds at most one
# path, and the /proc test reads only a 6-byte prefix.
KERNEL_TRACE = r"""
tracepoint:syscalls:sys_enter_openat /strncmp(comm, "dytallix", 8) == 0/ {
  @path[tid] = str(args->filename);
  @proc[tid] = 0;
  if (strncmp(str(args->filename, 7), "/proc/", 6) == 0) { @proc[tid] = 1; } }
tracepoint:syscalls:sys_exit_openat /@path[tid] != ""/ {
  printf("%s[%d] openat %s = %d\n", comm, tid, @path[tid], args->ret); delete(@path[tid]); }
tracepoint:raw_syscalls:sys_exit /strncmp(comm, "dytallix", 8) == 0 && args->ret < 0 && args->ret > -4096/ {
  printf("%s[%d] syscall %d = %d\n", comm, tid, args->id, args->ret); }
tracepoint:sched:sched_process_exit /strncmp(comm, "dytallix", 8) == 0/ { printf("%s[%d] exit\n", comm, pid); }
"""
# No strings at all: failing calls (errno) and exits, if the others do not load.
KERNEL_TRACE_ERRORS = r"""
tracepoint:raw_syscalls:sys_exit /strncmp(comm, "dytallix", 8) == 0 && args->ret < 0 && args->ret > -4096/ {
  printf("%s[%d] syscall %d = %d\n", comm, tid, args->id, args->ret); }
tracepoint:sched:sched_process_exit /strncmp(comm, "dytallix", 8) == 0/ { printf("%s[%d] exit\n", comm, pid); }
"""
KERNEL_TRACE_READS = r"""
tracepoint:syscalls:sys_enter_read /strncmp(comm, "dytallix", 8) == 0/ { @buf[tid] = (uint64)args->buf; }
tracepoint:syscalls:sys_exit_read /@buf[tid] != 0/ {
  if (args->ret > 0 && args->ret <= 96 && @proc[tid] == 1) {
    printf("%s[%d] read %d %r\n", comm, tid, args->ret, buf(uptr((uint8 *)@buf[tid]), 96)); }
  delete(@buf[tid]); }
"""


def kernel_traced_start():
    """Start the unit once more, with its whole sandbox, under bpftrace: every
    failing system call (errno), every open, and small /proc reads such as
    the AppArmor label the supervisor sees."""
    if not shutil.which('bpftrace'):
        say('(no kernel trace: bpftrace is missing)')
        return
    script = Path('/tmp/dytallix-start.bt')
    for body in (KERNEL_TRACE + KERNEL_TRACE_READS, KERNEL_TRACE, KERNEL_TRACE_ERRORS):
        script.write_text(body)
        say(f'$ bpftrace {script}; systemctl start {UNIT}.service')
        tracer = subprocess.Popen(['bpftrace', str(script)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                  text=True, env=dict(os.environ, BPFTRACE_MAX_STRLEN='96'))
        time.sleep(10)  # probe attachment
        if tracer.poll() is None:
            break
        # Fall back to fewer probes.
        say(f'bpftrace stopped: {tracer.communicate()[0][-1500:]}')
    else:
        return
    subprocess.run(['systemctl', 'start', f'{UNIT}.service'], check=False)
    time.sleep(5)
    tracer.send_signal(signal.SIGINT)
    try:
        output, _ = tracer.communicate(timeout=30)
    except subprocess.TimeoutExpired:
        tracer.kill()
        output, _ = tracer.communicate()
    lines = [line for line in output.splitlines() if not line.startswith('@')]
    say(f'-- last {min(len(lines), 300)} of {len(lines)} kernel trace lines --')
    for line in lines[-300:]:
        say(line)


def install(work, label, target_height, timeout):
    if os.geteuid() != 0:
        raise Failed('run install as root')
    work = Path(work)
    codes = json.loads((work / 'seal-codes.json').read_bytes())
    bundle = unpack(work, label)
    say(f'== install {label} (the seal code is typed on standard input)')
    result = subprocess.run([str(bundle / 'install.sh')], input=codes[label] + '\n', text=True)
    if result.returncode != 0:
        raise Failed('install.sh failed')
    say(f'== start {UNIT}')
    subprocess.run(['systemctl', 'start', f'{UNIT}.service'], check=True)
    seen = wait_for(target_height, timeout)
    verified = subprocess.run([sys.executable, '-I', '-B', str(bundle / 'host_install.py'), 'verify'])
    if verified.returncode != 0:
        diagnostics()
        raise Failed('the installed host does not verify')
    say(f'== restart at height {seen}: the node must rejoin and make blocks')
    subprocess.run(['systemctl', 'stop', f'{UNIT}.service'], check=True)
    subprocess.run(['systemctl', 'start', f'{UNIT}.service'], check=True)
    seen = wait_for(seen + 2, timeout)
    subprocess.run(['journalctl', '-u', f'{UNIT}.service', '--no-pager', '-n', '40'])
    say(f'== {label} installed, verified, restarted and at height {seen}')


def wait_for(target_height, timeout):
    """The height once it reaches the target; fails if the unit stops."""
    deadline, seen = time.monotonic() + timeout, None
    while time.monotonic() < deadline:
        current = height()
        if current != seen:
            say(f'height {current}')
            seen = current
        if current is not None and current >= target_height:
            return current
        active = subprocess.run(['systemctl', 'is-active', f'{UNIT}.service'], capture_output=True, text=True).stdout.strip()
        if active in ('failed', 'inactive'):
            diagnostics()
            raise Failed(f'{UNIT} is {active}')
        time.sleep(2)
    diagnostics()
    raise Failed(f'the node did not reach height {target_height} in {timeout} s')


def wipe(work, label):
    if os.geteuid() != 0:
        raise Failed('run wipe as root')
    bundle = unpack(work, label)
    say(f'== wipe {label}')
    result = subprocess.run([str(bundle / 'wipe.sh')], input=f'wipe {label}\n', text=True)
    if result.returncode != 0:
        raise Failed('wipe.sh failed')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    p = commands.add_parser('prepare')
    p.add_argument('--release', type=Path, required=True)
    p.add_argument('--work', type=Path, required=True)
    p.add_argument('--address', help="this host's address (default: the address it routes from)")
    for name in ('install', 'wipe'):
        c = commands.add_parser(name)
        c.add_argument('--work', type=Path, required=True)
        c.add_argument('--label', default='validator-1')
        if name == 'install':
            c.add_argument('--height', type=int, default=3)
            c.add_argument('--timeout', type=int, default=300)
    args = parser.parse_args()
    try:
        if args.command == 'prepare':
            print(json.dumps(prepare(args.release, args.work, args.address or host_address()), indent=2))
        elif args.command == 'install':
            install(args.work, args.label, args.height, args.timeout)
        else:
            wipe(args.work, args.label)
    except (Failed, OSError, KeyError, ValueError, subprocess.CalledProcessError) as error:
        print(f'staging_host: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
