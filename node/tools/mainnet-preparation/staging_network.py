#!/usr/bin/env python3
"""Run the two-seat staging network on one Ubuntu 24.04 CI runner (E05;
host setup v1, step H4; P01, 11 October 2026): the validator on the runner
itself, and the sentry, the one public node that also serves the client
channel and the status page, in a KVM virtual machine on a private bridge.
Each host has its own address on its own interface, as host setup v1
requires, and each installs from its own bundle with its install.sh.
Invented records and throwaway keys only.

  staging_network.py prepare --release DIR --work DIR
  sudo staging_network.py network-up --work DIR
  sudo staging_network.py vms-up --work DIR --image UBUNTU_24.04_CLOUD_IMAGE
  sudo staging_network.py install-vms --work DIR
  sudo staging_host.py install --work DIR/net --label validator-1 ...
  sudo staging_network.py run --work DIR
  sudo staging_network.py wallet-drill --work DIR
  sudo staging_network.py reconcile --work DIR
  sudo staging_network.py load --work DIR
  sudo staging_network.py reconcile --work DIR --after load
  sudo staging_network.py validator-recovery-drill --work DIR
  sudo staging_network.py restore-drill --work DIR
  sudo staging_network.py kit-drill --work DIR
  sudo staging_network.py monitor-drill --work DIR
  sudo staging_network.py agent-drill --work DIR
  sudo staging_network.py diagnostics --work DIR

The VM is driven through the QEMU guest agent (a virtio serial channel):
once a node is installed, its firewall drops every other inbound packet.
The bundle reaches it on its cloud-init disk. The VM installs its packages
and its node before the validator's install on the runner replaces the
runner's firewall rules (and with them the VM's route out).

run starts the sentry and checks that it follows the validator, that its
status page answers, that it takes a snapshot (a staging interval of 10
blocks), that its backup job uploads an encrypted copy to a stand-in store
inside the sentry VM, that the copy opens on the runner with the backup code
and holds the sentry's snapshot, and that the VM verifies against its
bundle.

reconcile is the reconciliation report (simulation plan, T6; reconcile.py)
after the wallet drill: both nodes' block and application hashes and supply
at a common height, every rehearsal account summed against the liquid
totals, the burns against the drill's ledger, and the sentry's database
checked on a stopped copy. Anything but PASS fails the step; the evidence is
reconcile/evidence.json. With --after load it reconciles the load step
instead: from the load's first status view, against its ledger, with every
account the load created tracked too; its evidence is reconcile-load/evidence.json.

load is the load generator (simulation plan, T3; load.py), a short CI run:
16 load wallets, a governance proposer, depositor and voter, 8 spare keys
and 2 distributors, made with the release's CLI and funded from the
ecosystem growth account through the distributors; 1 transaction a second
for 2 minutes in the default mix over the sentry's client channel, with the
governance steps beside it and one reward sweep back to the funder. Anything but
PASS fails the step; the evidence is load/run/evidence.json.

checkpoint is the checkpoint recorder (simulation plan, T7; checkpoints.py)
after the after-load reconciliation: a short schedule of the R07 checks CI can
source, recorded against that fresh reconciliation and the load run (each
bound by digest and re-verified) and the client channel. The record verifies
to PASS and is refused as the R07 dress rehearsal; its evidence is
checkpoint/evidence.json.

validator-recovery-drill is the validator recovery drill (simulation plan,
T9; validator recovery F17): the validator's host is lost, fenced (the
runner's wipe.sh stands for the provider deleting it), the sentry's old votes
set aside, and the replacement installed from its bundle; it block-syncs to
the halt height H from the sentry and signs from H + 1. The evidence
(recovery.py) is judged against the approved 8-hour objective; it is
validator-recovery/evidence.json. It runs before restore-drill, while the
sentry still holds every block from genesis.

restore-drill is the first restore drill (disaster recovery v1, R6): the
sentry is lost, so its VM is wiped and reinstalled from its bundle, then
restored from the stand-in store's copy with restore.sh and the backup code
typed from paper. It must catch up with the validator, at the same height
with the same block and application hashes, and verify against its bundle.
It prints the measured times.

kit-drill is the kit replacement drill (root kit replacement v1, K4; P01, 8
October 2026), run as an operator would with the release's own tools from
the runner, over the sentry's client channel: seat 5's kit is replaced by
a new kit, signed by three kits' upgrade keys and the new kit's three keys.
After the staging notice of 20 blocks the old kit is refused (by the signer,
and, signed from a stale key list, by assembly and the node) and the new kit
freezes and resumes the chain. It prints the evidence.

monitor-drill is the monitoring drill (monitoring v1, M5): the sentry's
monitor reports to a stand-in alerting service on its loopback, with the
approved rules. A restart loop (three restarts of its node), a full disk
(the data disk under the critical level), a halt (the validator stopped) and
a stopped monitor (its timer stopped; only the service sees it) must each
alert and then resolve, and nothing may be firing after. It prints the
evidence.

agent-drill is the staging agent drill (simulation plan, T2): the founder's
staging command key is made with the release's CLI on the runner and the
agent installed on the sentry beside its node. Signed status and journal
commands run; a tampered, an unsigned, a replayed, a non-allowlisted and an
expired command are refused, and the node keeps running. The reconciliation
(T6) then runs through the agent's reconcile-read, as on console-only hosts;
its evidence is reconcile-agent/evidence.json. Every result is
encrypted with the history code, uploaded to the stand-in store and opened
on the runner. Last, wipe.sh removes the node and the agent together.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shutil
import socket
import subprocess
import sys
import time
import urllib.request

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import adversary  # noqa: E402
import checkpoints  # noqa: E402
import faults  # noqa: E402
import load as load_generator  # noqa: E402
import reconcile  # noqa: E402
import recovery  # noqa: E402
import staging_command  # noqa: E402
import staging_founder  # noqa: E402
import staging_host  # noqa: E402

BRIDGE = 'dytbr0'
ADDRESSES = {'validator-1': '10.77.0.1', 'sentry-1': '10.77.0.11'}
VMS = ('sentry-1',)
# The public node of two seats: the client channel and the status page.
PUBLIC = 'sentry-1'
MACS = {'sentry-1': '52:54:00:77:00:11'}
TAPS = {'sentry-1': 'dyttap11'}
SNAPSHOT_INTERVAL = 10
STANDIN_PORT = 9000
UPLOAD = {'schema': 'dytallix.backup-upload.v1', 'endpoint': f'http://127.0.0.1:{STANDIN_PORT}',
          'bucket': 'copies', 'region': 'auto', 'prefix': 'staging/', 'access_key_id': 'STANDIN',
          'secret_access_key': 'c3RhbmRpbi1vbmx5'}
# A separate stand-in key for the hosts' metric history (M4b): every host
# seals and installs it; only a finished day is ever uploaded.
HISTORY_UPLOAD = dict(UPLOAD, prefix='staging-history/', access_key_id='STANDINHISTORY',
                      secret_access_key='aGlzdG9yeS1vbmx5')
METRICS = '/var/lib/dytallix/metrics/dytallix-app.prom'
SNAPSHOTS = '/var/lib/dytallix/snapshots'
SNAPSHOT_LIGHT_BLOCKS = '/var/lib/dytallix/snapshot-light-blocks'
HEIGHT = re.compile(r'^dytallix_app_height (\d+)$', re.M)


class Failed(Exception):
    pass


def say(message):
    print(message, flush=True)


def sh(*args, check=True):
    say('$ ' + ' '.join(args))
    result = subprocess.run(args, capture_output=True, text=True)
    if result.stdout.strip():
        say(result.stdout.rstrip())
    if check and result.returncode != 0:
        raise Failed(f'{args[0]} failed: {result.stderr.strip()}')
    return result


# The plan and the VMs' cloud-init

def network_plan():
    plan = json.loads(staging_host.TEMPLATE.read_bytes())
    plan['chain_id'] = staging_host.CHAIN_ID
    for host in plan['hosts']:
        ip = ADDRESSES[host['label']]
        for field in ('p2p', 'channel', 'status'):
            if host[field]:
                host[field] = ip + host[field][host[field].rindex(':'):]
    return plan


def user_data(label):
    return '\n'.join([
        '#cloud-config',
        f'hostname: {label}',
        'package_update: true',
        'packages: [qemu-guest-agent, apparmor-utils, nftables]',
        'runcmd:',
        '  - [systemctl, enable, --now, qemu-guest-agent]',
        '  - [sh, -c, "ufw disable || true"]',
        '  - [chmod, "0755", /opt]',
        '']).encode()


def network_config(label):
    return '\n'.join([
        'version: 2',
        'ethernets:',
        '  nic0:',
        f'    match: {{macaddress: "{MACS[label]}"}}',
        '    set-name: eth0',
        f'    addresses: [{ADDRESSES[label]}/24]',
        f'    routes: [{{to: default, via: {ADDRESSES["validator-1"]}}}]',
        '    nameservers: {addresses: [1.1.1.1, 8.8.8.8]}',
        '']).encode()


def meta_data(label):
    return f'instance-id: {label}\nlocal-hostname: {label}\n'.encode()


# The QEMU guest agent

class Agent:
    """A QEMU guest agent client on the VM's virtio serial socket."""

    def __init__(self, path):
        self.path = str(path)

    def call(self, command, arguments=None, timeout=30):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(timeout)
            sock.connect(self.path)
            stream = sock.makefile('rwb')
            token = random.randrange(1, 2**31)
            stream.write(json.dumps({'execute': 'guest-sync', 'arguments': {'id': token}}).encode() + b'\n')
            stream.flush()
            while True:
                line = stream.readline()
                if not line:
                    raise Failed('the guest agent closed the channel')
                try:
                    if json.loads(line.strip(b'\xff')).get('return') == token:
                        break
                except ValueError:
                    continue
            request = {'execute': command}
            if arguments is not None:
                request['arguments'] = arguments
            stream.write(json.dumps(request).encode() + b'\n')
            stream.flush()
            reply = json.loads(stream.readline())
        if 'error' in reply:
            raise Failed(f'{command}: {reply["error"].get("desc")}')
        return reply.get('return')

    def wait(self, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                self.call('guest-ping', timeout=5)
                return
            except (OSError, Failed, ValueError):
                time.sleep(3)
        raise Failed(f'no guest agent on {self.path} after {timeout} s')

    def run(self, args, stdin=None, timeout=600, check=True):
        arguments = {'path': args[0], 'arg': list(args[1:]), 'capture-output': True}
        if stdin is not None:
            arguments['input-data'] = base64.b64encode(stdin.encode()).decode()
        pid = self.call('guest-exec', arguments)['pid']
        deadline = time.monotonic() + timeout
        while True:
            status = self.call('guest-exec-status', {'pid': pid})
            if status.get('exited'):
                break
            if time.monotonic() > deadline:
                raise Failed(f'{" ".join(args)} did not finish in {timeout} s')
            time.sleep(1)
        out = base64.b64decode(status.get('out-data', '')).decode(errors='replace')
        err = base64.b64decode(status.get('err-data', '')).decode(errors='replace')
        code = status.get('exitcode', -1)
        if check and code != 0:
            raise Failed(f'{" ".join(args)} exited {code}:\n{out}\n{err}')
        return code, out, err

    def read(self, path):
        handle = self.call('guest-file-open', {'path': path, 'mode': 'r'})
        data = bytearray()
        try:
            while True:
                chunk = self.call('guest-file-read', {'handle': handle, 'count': 1 << 20}, timeout=60)
                data += base64.b64decode(chunk['buf-b64'])
                if chunk.get('eof'):
                    break
        finally:
            self.call('guest-file-close', {'handle': handle})
        return bytes(data)


def agent(work, label):
    return Agent(Path(work) / 'vm' / f'{label}.qga')


# Steps

def prepare(release, work):
    work = Path(work)
    work.mkdir(parents=True)
    upload = work / 'upload.json'
    descriptor = os.open(upload, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        json.dump(UPLOAD, file)
    history = work / 'history-upload.json'
    descriptor = os.open(history, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        json.dump(HISTORY_UPLOAD, file)
    return staging_host.prepare(release, work / 'net', plan=network_plan(), backup_upload=upload,
                                history_upload=history, snapshot_interval=SNAPSHOT_INTERVAL)


def network_up(work):
    sh('ip', 'link', 'add', BRIDGE, 'type', 'bridge')
    sh('ip', 'addr', 'add', f'{ADDRESSES["validator-1"]}/24', 'dev', BRIDGE)
    sh('ip', 'link', 'set', BRIDGE, 'up')
    for label in VMS:
        sh('ip', 'tuntap', 'add', 'dev', TAPS[label], 'mode', 'tap')
        sh('ip', 'link', 'set', TAPS[label], 'master', BRIDGE)
        sh('ip', 'link', 'set', TAPS[label], 'up')
    # A route out for the VMs' packages, until the validator's install
    # replaces the runner's rules.
    sh('sysctl', '-w', 'net.ipv4.ip_forward=1')
    sh('iptables', '-t', 'nat', '-A', 'POSTROUTING', '-s', '10.77.0.0/24', '!', '-o', BRIDGE, '-j', 'MASQUERADE')
    sh('iptables', '-I', 'FORWARD', '-i', BRIDGE, '-j', 'ACCEPT')
    sh('iptables', '-I', 'FORWARD', '-o', BRIDGE, '-m', 'conntrack', '--ctstate', 'RELATED,ESTABLISHED', '-j', 'ACCEPT')


def vms_up(work, image):
    work = Path(work)
    require_kvm()
    vm = work / 'vm'
    vm.mkdir()
    for label in VMS:
        seed = vm / f'{label}-seed'
        seed.mkdir()
        (seed / 'user-data').write_bytes(user_data(label))
        (seed / 'meta-data').write_bytes(meta_data(label))
        (seed / 'network-config').write_bytes(network_config(label))
        shutil.copyfile(work / 'net' / 'chain' / 'bundles' / f'{label}.bundle.tar', seed / 'bundle.tar')
        for name in ('s3_standin.py', 'alert_standin.py'):
            shutil.copyfile(HERE / name, seed / name)
        sh('genisoimage', '-quiet', '-output', str(vm / f'{label}-seed.iso'), '-volid', 'cidata', '-joliet', '-rock',
           *[str(seed / name) for name in ('user-data', 'meta-data', 'network-config', 'bundle.tar', 's3_standin.py',
                                                    'alert_standin.py')])
        sh('qemu-img', 'create', '-q', '-f', 'qcow2', '-F', 'qcow2', '-b', str(Path(image).resolve()),
           str(vm / f'{label}.qcow2'), '20G')
        sh('qemu-system-x86_64', '-name', label, '-machine', 'q35,accel=kvm', '-cpu', 'host', '-smp', '2', '-m', '4096',
           '-drive', f'file={vm / f"{label}.qcow2"},if=virtio,format=qcow2',
           '-drive', f'file={vm / f"{label}-seed.iso"},if=virtio,format=raw,readonly=on',
           '-netdev', f'tap,id=net0,ifname={TAPS[label]},script=no,downscript=no',
           '-device', f'virtio-net-pci,netdev=net0,mac={MACS[label]}',
           '-chardev', f'socket,path={vm / f"{label}.qga"},server=on,wait=off,id=qga0',
           '-device', 'virtio-serial', '-device', 'virtserialport,chardev=qga0,name=org.qemu.guest_agent.0',
           '-display', 'none', '-serial', f'file:{vm / f"{label}.console.log"}',
           '-daemonize', '-pidfile', str(vm / f'{label}.pid'))
    for label in VMS:
        say(f'== waiting for {label}\'s guest agent (cloud-init installs it)')
        guest = agent(work, label)
        guest.wait(900)
        code, out, _ = guest.run(['cloud-init', 'status', '--wait'], timeout=900, check=False)
        say(out.strip())
        if code not in (0, 2):
            raise Failed(f'{label}: cloud-init failed')


def require_kvm():
    if not os.path.exists('/dev/kvm'):
        raise Failed('this runner has no /dev/kvm; the sentry VM needs KVM')


def wait_synchronized(guest, label, timeout=300):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        _, out, _ = guest.run(['timedatectl', 'show', '-p', 'NTPSynchronized', '--value'], check=False)
        if out.strip() == 'yes':
            return
        time.sleep(5)
    raise Failed(f'{label}: the clock did not synchronize')


def install_vms(work):
    work = Path(work)
    codes = json.loads((work / 'net' / 'seal-codes.json').read_bytes())
    for label in VMS:
        guest = agent(work, label)
        wait_synchronized(guest, label)
        guest.run(['sh', '-c', 'mkdir -p /mnt/seed /root/node && mount -o ro /dev/disk/by-label/cidata /mnt/seed '
                   '&& tar -xf /mnt/seed/bundle.tar -C /root/node && cp /mnt/seed/s3_standin.py /mnt/seed/alert_standin.py /root/ '
                   '&& umount /mnt/seed'])
        say(f'== install {label} (typing its seal code)')
        _, out, err = guest.run(['/root/node/dytallix-host/install.sh'], stdin=codes[label] + '\n', timeout=900)
        say(out.strip())
        if err.strip():
            say(err.strip())


def guest_height(guest):
    code, out, _ = guest.run(['cat', METRICS], check=False)
    found = HEIGHT.search(out) if code == 0 else None
    return int(found.group(1)) if found else None


def wait_height(guest, label, target, timeout):
    deadline, seen = time.monotonic() + timeout, None
    while time.monotonic() < deadline:
        current = guest_height(guest)
        if current != seen:
            say(f'{label} height {current}')
            seen = current
        if current is not None and current >= target:
            return current
        _, state, _ = guest.run(['systemctl', 'is-active', 'dytallix-node'], check=False)
        if state.strip() in ('failed', 'inactive'):
            raise Failed(f'{label}: dytallix-node is {state.strip()}')
        time.sleep(3)
    raise Failed(f'{label} did not reach height {target} in {timeout} s')


def monitor_run(guest, label):
    """One run of the host's monitor job in its sandbox (monitoring v1, M4):
    before its settings exist it reads every input and sends nothing. The
    stand-in store is the sentry's own loopback, so a history upload (only
    after a UTC midnight) may fail here; any other error fails the run."""
    guest.run(['systemctl', 'start', 'dytallix-monitor.service'], timeout=180)
    _, journal, _ = guest.run(['journalctl', '-u', 'dytallix-monitor', '-o', 'cat', '--no-pager', '-n', '20'])
    runs = [json.loads(line) for line in journal.splitlines() if line.startswith('{"status"')]
    errors = [e for e in runs[-1]['errors'] if not e.startswith('history: the upload')] if runs else None
    if not runs or runs[-1]['heartbeat'] != 'NO_SETTINGS' or errors or runs[-1]['history_pending'] is None:
        raise Failed(f'{label}: the monitor job did not run cleanly:\n{journal.strip()[-2000:]}')
    say(f'== {label} monitor: {json.dumps(runs[-1])}')


def run(work, timeout=600):
    work = Path(work)
    guests = {label: agent(work, label) for label in VMS}
    for label, guest in guests.items():
        say(f'== start {label}')
        guest.run(['systemctl', 'start', 'dytallix-node'])
    validator = staging_host.height() or 0
    target = max(validator, 2 * SNAPSHOT_INTERVAL)
    for label, guest in guests.items():
        wait_height(guest, label, target, timeout)
    # The public node's status page, from the runner.
    with urllib.request.urlopen(f'http://{ADDRESSES[PUBLIC]}:8080/status', timeout=10) as response:
        page = json.loads(response.read())
    if page.get('chain_id') != staging_host.CHAIN_ID or not int(page.get('height', 0)) > 0:
        raise Failed(f'the {PUBLIC} status page is wrong: {page}')
    say(f'== {PUBLIC} status page: {page}')
    for label, guest in guests.items():
        monitor_run(guest, label)
    # The sentry's snapshot and its off-host copy.
    sentry = guests['sentry-1']
    deadline = time.monotonic() + 180
    while True:
        _, listing, _ = sentry.run(['sh', '-c', f'cd {SNAPSHOTS} && ls -d [0-9]*/metadata.json 2>/dev/null'], check=False)
        heights = sorted(int(name.split('/')[0]) for name in listing.split())
        # The engine writes a snapshot's light blocks once H+2 commits (R4).
        _, listing, _ = sentry.run(['sh', '-c', f'cd {SNAPSHOT_LIGHT_BLOCKS} && ls -d [0-9]* 2>/dev/null'], check=False)
        light = sorted(int(name) for name in listing.split())
        if set(heights) & set(light):
            say(f'== sentry snapshots at {heights}, their light blocks at {light}')
            break
        if time.monotonic() > deadline:
            raise Failed(f'the sentry has no snapshot with its light blocks (snapshots {heights}, light blocks {light})')
        time.sleep(5)
    sentry.run(['systemd-run', '--unit=dytallix-store-standin', 'python3', '/root/s3_standin.py', '--root',
                '/var/tmp/standin', '--port', str(STANDIN_PORT), '--access-key-id', UPLOAD['access_key_id']])
    time.sleep(2)
    sentry.run(['systemctl', 'start', 'dytallix-backup.service'], timeout=900)
    _, journal, _ = sentry.run(['journalctl', '-u', 'dytallix-backup', '-o', 'cat', '--no-pager'])
    say(journal.strip())
    uploaded = [json.loads(line) for line in journal.splitlines() if line.startswith('{') and '"UPLOADED"' in line]
    if not uploaded:
        raise Failed('the sentry\'s backup job uploaded nothing')
    copy = uploaded[-1]
    # The restore drill takes the same copy from the store.
    (work / 'backup-uploaded.json').write_text(json.dumps(copy))
    stored = f'/var/tmp/standin/{UPLOAD["bucket"]}/{copy["object"]}'
    data = sentry.read(stored)
    local = work / 'backup-copy.bin'
    local.write_bytes(data)
    codes = json.loads((work / 'net' / 'seal-codes.json').read_bytes())
    paper = work / 'backup-paper.txt'
    descriptor = os.open(paper, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        file.write(codes['backup'] + '\n')
    signer = Path(work) / 'release-bin' / 'dytallix-root-sign'
    restored = work / 'restored'
    sh(str(signer), 'backup-open', '-paper', str(paper), '-in', str(local), '-out', str(restored))
    remote = sentry.read(f'{SNAPSHOTS}/{copy["height"]:020}/metadata.json')
    if (restored / 'metadata.json').read_bytes() != remote:
        raise Failed('the restored copy is not the sentry\'s snapshot')
    # The copy carries the light blocks its restore verifies (R4): heights H to
    # H+2, as the sentry's engine wrote them.
    height = copy['height']
    names = sorted(f'{h:020}.{kind}' for h in range(height, height + 3) for kind in ('block', 'params'))
    found = sorted(p.name for p in (restored / 'light-blocks').iterdir()) if (restored / 'light-blocks').is_dir() else []
    if found != names:
        raise Failed(f'the copy\'s light blocks are {found}, not heights {height} to {height + 2}')
    for name in names:
        if (restored / 'light-blocks' / name).read_bytes() != sentry.read(f'{SNAPSHOT_LIGHT_BLOCKS}/{height:020}/{name}'):
            raise Failed(f'the copy\'s {name} is not the sentry\'s')
    say(f'== backup of height {height} uploaded, opened with the backup code and matches the snapshot and its light blocks')
    for label, guest in guests.items():
        _, out, _ = guest.run(['python3', '-I', '-B', '/root/node/dytallix-host/host_install.py', 'verify'])
        say(f'{label}: {out.strip()}')
    say('== the staging network runs: the validator and the public sentry, with an off-host copy')


# The first restore drill (disaster recovery v1, R6)

OPERATOR_STATUS = reconcile.OPERATOR_STATUS


def sync_info(raw):
    """(height, block hash, application hash) from the operator status."""
    info = json.loads(raw)['result']['sync_info']
    return int(info['latest_block_height']), info['latest_block_hash'], info['latest_app_hash']


def restore_drill(work, timeout=900):
    work = Path(work)
    sentry = agent(work, 'sentry-1')
    codes = json.loads((work / 'net' / 'seal-codes.json').read_bytes())
    copy = json.loads((work / 'backup-uploaded.json').read_bytes())
    restored = json.loads((work / 'restored' / 'metadata.json').read_bytes())
    say(f'== the copy: height {copy["height"]}, block records from height {restored.get("retained_from")}')
    # The copy as the founder fetches it: a file outside the node's trees.
    sentry.run(['cp', f'/var/tmp/standin/{UPLOAD["bucket"]}/{copy["object"]}', '/root/restore-copy.bin'])
    times, started = {}, time.monotonic()

    def lap(name, since):
        times[name] = round(time.monotonic() - since, 1)
        return time.monotonic()

    say('== the sentry is lost: wipe it')
    _, out, _ = sentry.run(['/root/node/dytallix-host/wipe.sh'], stdin='wipe sentry-1\n', timeout=300)
    say(out.strip())
    mark = lap('wipe_seconds', started)
    say('== reinstall it from its bundle (typing its seal code)')
    _, out, _ = sentry.run(['/root/node/dytallix-host/install.sh'], stdin=codes['sentry-1'] + '\n', timeout=900)
    say(out.strip())
    mark = lap('install_seconds', mark)
    say('== restore it from the copy (typing the backup code)')
    code, out, err = sentry.run(['/root/node/dytallix-host/restore.sh', '/root/restore-copy.bin'],
                                stdin=codes['backup'] + '\n', timeout=timeout, check=False)
    say(out.strip())
    if code != 0:
        raise Failed(f'restore.sh exited {code}: {err.strip()}')
    mark = lap('restore_and_catch_up_seconds', mark)
    # Caught up: the same height, block hash and application hash as the
    # validator, read from both engines' operator status.
    deadline = time.monotonic() + 180
    while True:
        _, raw, _ = sentry.run(list(OPERATOR_STATUS))
        ours = sync_info(raw)
        theirs = sync_info(subprocess.run(OPERATOR_STATUS, capture_output=True, text=True, check=True).stdout)
        if ours[0] == theirs[0]:
            if ours != theirs:
                raise Failed(f'the restored sentry differs from the validator at height {ours[0]}: {ours} vs {theirs}')
            break
        if time.monotonic() > deadline:
            raise Failed(f'the restored sentry is at {ours[0]}, the validator at {theirs[0]}')
        time.sleep(1)
    times['total_seconds'] = round(time.monotonic() - started, 1)
    _, out, _ = sentry.run(['python3', '-I', '-B', '/root/node/dytallix-host/host_install.py', 'verify'])
    say(f'sentry-1: {out.strip()}')
    say(json.dumps({'restore_drill': {'copy_height': copy['height'], 'compared_height': ours[0],
                                      'block_hash': ours[1], 'app_hash': ours[2], **times}}, indent=2))
    say(f'== the sentry was wiped, reinstalled and restored from the copy of height {copy["height"]}, '
        f'and matches the validator at height {ours[0]}')


# The validator recovery drill (simulation plan, T9; validator recovery F17)

WAL = '/var/lib/dytallix/node/data/cs.wal'


def validator_recovery_drill(work, timeout=900):
    """The validator's host is lost and replaced by the fence-first procedure
    (node/docs/operations/validator-recovery.md), timed on the runner's clock:
    the runner's wipe.sh stands for the provider deleting the old server (the
    fence); the sentry's height is read twice to confirm the halt at H; on the
    sentry the old votes are set aside (its consensus log moved, never
    deleted); the replacement is installed from the same bundle with its seal
    code typed and not started; the sentry, then the validator, start; the
    validator block-syncs to H from the sentry and signs from H + 1. It writes
    digest-bound evidence (recovery.py) judged against the approved 8-hour
    objective. Runs before the restore drill, while the sentry still holds
    every block from genesis for the replacement to sync."""
    work = Path(work)
    out = work / 'validator-recovery'
    sentry = agent(work, PUBLIC)
    codes = json.loads((work / 'net' / 'seal-codes.json').read_bytes())
    client = reconcile.Client(work / 'release-bin' / 'dytallix', pinned_cli(work, out), out)
    validators = lambda view: view.get('effective_validators')  # noqa: E731
    obs = {'clock': 'the runner', 'instants': {}, 'validators_before': validators(client.status())}
    instants = obs['instants']

    def engine(host):
        raw = (host.run(list(OPERATOR_STATUS), check=False)[1] if host else
               subprocess.run(OPERATOR_STATUS, capture_output=True, text=True).stdout)
        try:
            return sync_info(raw)
        except (ValueError, KeyError, TypeError):
            return None

    instants['declared_at'] = time.time()
    say('== the validator\'s host is lost: fence it (wipe.sh stands for the provider deleting it)')
    staging_host.wipe(work / 'net', 'validator-1')
    instants['fenced_at'] = time.time()
    first = engine(sentry)
    time.sleep(15)
    second = engine(sentry)
    obs['halt_readings'] = [r[0] if r else None for r in (first, second)]
    halt = obs['halt_readings'][-1]
    say(f'== the sentry stayed at {obs["halt_readings"]}: H = {halt}')
    say('== on the sentry, set the old votes aside')
    sentry.run(['systemctl', 'stop', 'dytallix-node'], timeout=120)
    aside = f'{WAL}.f17-{time.strftime("%Y%m%d%H%M%S", time.gmtime())}'
    sentry.run(['mv', WAL, aside])
    sentry.run(['install', '-d', '-o', 'dytallix', '-g', 'dytallix', '-m', '0700', WAL])
    obs['wal_set_aside'] = {'host': PUBLIC, 'moved_to': aside}
    instants['wal_set_aside_at'] = time.time()
    say('== install the replacement from its bundle (typing its seal code); do not start it')
    bundle = staging_host.unpack(work / 'net', 'validator-1')
    result = subprocess.run([str(bundle / 'install.sh')], input=codes['validator-1'] + '\n', text=True)
    if result.returncode != 0:
        raise Failed('install.sh failed on the replacement')
    instants['installed_at'] = time.time()
    sentry.run(['systemctl', 'start', 'dytallix-node'], timeout=120)
    instants['sentry_started_at'] = time.time()
    subprocess.run(['systemctl', 'start', 'dytallix-node.service'], check=True)
    instants['validator_started_at'] = time.time()
    deadline = time.monotonic() + timeout
    while True:
        ours = engine(None)
        if ours and halt is not None and ours[0] > halt:
            instants['first_block_after_halt_at'] = time.time()
            say(f'== the replacement made height {ours[0]}, past H = {halt}')
            break
        if time.monotonic() > deadline:
            raise Failed(f'the replacement did not pass H = {halt} within {timeout} s (it is at {ours})')
        time.sleep(1)
    # Signing again, the sentry following, both at one height with one state.
    target = halt + recovery.AFTER
    while True:
        theirs, ours = engine(sentry), engine(None)
        if ours and theirs and min(ours[0], theirs[0]) >= target and ours[0] == theirs[0]:
            break
        if time.monotonic() > deadline:
            raise Failed(f'the nodes did not both reach {target} together: validator {ours}, sentry {theirs}')
        time.sleep(0.5)
    obs['reached'] = {'validator': ours[0], 'sentry': theirs[0]}
    obs['compared'] = {name: {'height': r[0], 'block_hash': r[1].lower(), 'app_hash': r[2].lower()}
                       for name, r in (('validator', ours), ('sentry', theirs))}
    obs['host_verify_exit'] = subprocess.run([sys.executable, '-I', '-B', str(bundle / 'host_install.py'),
                                              'verify']).returncode
    obs['validators_after'] = validators(client.status())
    genesis = (work / 'net' / 'chain' / 'chain' / 'native-genesis.json').read_bytes()
    record = recovery.build(staging_host.CHAIN_ID, genesis, obs, recovery.approved_target())
    path = out / 'evidence.json'
    path.write_text(json.dumps(record, indent=2) + '\n')
    for row in record['checks']:
        say(f'{row["status"]:<12} {row["check"]}: {row["detail"]}')
    took = {f'{a}->{b}': round(instants[b] - instants[a], 1) for a, b in zip(recovery.STEPS, recovery.STEPS[1:])}
    say(json.dumps({'validator_recovery_drill': {'halt_height': halt, 'status': record['status'], **took}}, indent=2))
    if record['status'] != recovery.PASS or recovery.verify(json.loads(path.read_bytes()), genesis) != recovery.PASS:
        raise Failed(f'the validator recovery drill is {record["status"]}')
    say(f'== the validator was fenced, replaced and signs again past H = {halt}; evidence sha256 {record["sha256"]}')


# The kit replacement drill (root kit replacement v1, K4)

SEAT = 5


def tool(binary, *args, env=None, quiet=False, check=True):
    """A release tool, as an operator runs it."""
    say('$ ' + ' '.join([Path(binary).name, *map(str, args)]))
    result = subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True, env=env)
    if result.stdout.strip() and not quiet:
        say(result.stdout.rstrip())
    if check and result.returncode != 0:
        raise Failed(f'{Path(binary).name} failed: {result.stderr.strip()}')
    return result


def with_signature(control, leaving, record):
    """A control with one signature swapped for another key's, in the
    node's canonical form, which is all the node decodes: the assembled
    control's key order, compact, its signatures sorted by key ID."""
    value = json.loads(control)
    signatures = [s for s in value['signatures'] if s['key_id'] != leaving]
    if len(signatures) != len(value['signatures']) - 1:
        raise Failed(f'the control has no signature by {leaving}')
    signatures.append({'key_id': record['key_id'], 'signature_hex': record['signature_hex']})
    value['signatures'] = sorted(signatures, key=lambda s: s['key_id'])
    return json.dumps(value, separators=(',', ':')).encode()


def kit_drill(work, timeout=900):
    work = Path(work)
    bins, chain = work / 'release-bin', work / 'net' / 'chain'
    config = chain / 'chain' / 'application-config.json'
    kits = chain / 'control-kits'
    drill = work / 'kit-drill'
    (drill / 'home').mkdir(parents=True)
    # The CLI's own home, with the public node pinned over its client channel.
    env = dict(os.environ, HOME=str(drill / 'home'))
    digest = hashlib.sha256((chain / 'chain' / 'native-genesis.json').read_bytes()).hexdigest()
    tool(bins / 'dytallix', 'config', 'pin-chain', '--endpoint', work / 'net' / 'keys' / f'{PUBLIC}.channel-pin.json',
         '--network', 'testnet', '--chain-id', staging_host.CHAIN_ID, '--genesis-digest', digest, env=env)
    saved = iter(range(1, 1_000_000))

    def status():
        path = drill / f'status-{next(saved)}.json'
        tool(bins / 'dytallix', 'control', 'status', '--out', path, env=env, quiet=True)
        return path, json.loads(path.read_bytes())

    def wait(what, done):
        deadline = time.monotonic() + timeout
        while True:
            path, view = status()
            if done(view):
                say(f'== {what}: height {view["height"]}')
                return path, view
            if time.monotonic() > deadline:
                raise Failed(f'{what} did not happen by height {view["height"]}')
            time.sleep(3)

    def prepare(operation, *inputs):
        path, _ = status()
        out = drill / f'{operation}-request.json'
        tool(bins / 'dytallix-control', 'prepare', operation, '--config', config, '--status', path,
             '--out', out, *inputs)
        tool(bins / 'dytallix-root-sign', 'show-control', '-request', out)
        return out, json.loads(out.read_bytes())

    def sign(request_path, request, private, public):
        out = drill / f'{request["operation"]}-{public.parent.name}-{public.stem}.sig.json'
        tool(bins / 'dytallix-root-sign', 'sign-control', '-request', request_path, '-private-key', private,
             '-public-key', public, '-operation', request['operation'],
             '-sequence', request['envelope']['sequence'], '-out', out, quiet=True)
        return out

    def kit_key(n, purpose):
        return kits / f'kit-{n}' / f'kit-{n}-{purpose}.key', kits / 'public' / f'kit-{n}-{purpose}.json'

    def new_key(purpose):
        return drill / 'new-kit' / f'kit-{SEAT}-{purpose}.key', drill / 'new-public' / f'kit-{SEAT}-{purpose}.json'

    def assemble(request_path, request, signatures):
        path, _ = status()
        control = drill / f'{request["operation"]}-control.json'
        tool(bins / 'dytallix-control', 'assemble', '--config', config, '--status', path,
             '--request', request_path, '--out', control, *signatures)
        return control

    def send(control, operation):
        for command, admitted in (('check', 'WOULD_BE_ADMITTED'), ('submit', 'SUBMITTED')):
            result = json.loads(tool(bins / 'dytallix', 'control', command, control, env=env).stdout)
            if result['status'] != admitted:
                raise Failed(f'the {operation} control was refused: {result}')
        return hashlib.sha256(control.read_bytes()).hexdigest()

    def submit(request_path, request, signatures):
        return send(assemble(request_path, request, signatures), request['operation'])

    _, before = status()
    authority = before['root_authority']
    if authority['authority_epoch'] != 1 or authority['pending'] or before['emergency_control']['frozen']:
        raise Failed(f'the drill needs epoch 1, nothing pending and no freeze: {authority}')

    say(f'== a new kit for seat {SEAT}, made as the key ceremony makes one')
    for name in ('new-kit', 'new-public'):
        (drill / name).mkdir(mode=0o700)
    tool(bins / 'dytallix-root-sign', 'kit', '-number', SEAT, '-private-out', drill / 'new-kit',
         '-public-out', drill / 'new-public', quiet=True)
    request_path, request = prepare('kit-replacement', '--seat', SEAT, '--old-kit', kits / 'public',
                                    '--new-kit', drill / 'new-public')
    signatures = [sign(request_path, request, *kit_key(n, 'upgrade')) for n in (1, 3, 4)]
    signatures += [sign(request_path, request, *new_key(purpose)) for purpose in ('upgrade', 'freeze', 'resume')]
    replacement = submit(request_path, request, signatures)
    _, admitted = wait('the replacement is admitted', lambda view: view['root_authority']['pending'])
    effect = admitted['root_authority']['pending']['effect_height']
    say(f'== admitted by height {admitted["height"]}; the new kit signs from height {effect}')

    _, switched = wait('the new epoch is in force', lambda view: view['root_authority']['authority_epoch'] == 2)
    incident = hashlib.sha256(b'kit replacement drill').hexdigest()
    request_path, request = prepare('freeze', '--incident-sha256', incident)
    # The old kit: the signer refuses its key, which the request no longer
    # lists.
    old_private, old_public = kit_key(SEAT, 'freeze')
    refused = tool(bins / 'dytallix-root-sign', 'sign-control', '-request', request_path, '-private-key', old_private,
                   '-public-key', old_public, '-operation', 'freeze', '-sequence', request['envelope']['sequence'],
                   '-out', drill / 'freeze-old-kit.sig.json', quiet=True, check=False)
    if refused.returncode == 0 or "is not one of this control's freeze keys" not in refused.stderr:
        raise Failed(f'the signer took the old kit\'s key: {refused.stderr.strip()}')
    signer_refused = refused.stderr.strip()
    say(f'== the signer refuses the old kit: {signer_refused}')
    # The list is the signer's guard, not part of what it signs: a signer
    # given a stale list signs the same envelope with the old key. Assembly
    # refuses that signature, and so does the node.
    stale = json.loads(request_path.read_bytes())
    arriving_id = json.loads(new_key('freeze')[1].read_bytes())['key_id']
    old_record = json.loads(old_public.read_bytes())
    stale['authority']['keys'] = sorted(
        [k for k in stale['authority']['keys'] if k['key_id'] != arriving_id] +
        [{'key_id': old_record['key_id'], 'public_key_hex': old_record['public_key_hex']}],
        key=lambda k: k['key_id'])
    stale_path = drill / 'freeze-stale-list-request.json'
    stale_path.write_text(json.dumps(stale, indent=2))
    old = [sign(request_path, request, *kit_key(n, 'freeze')) for n in (1, 2)]
    old.append(sign(stale_path, request, old_private, old_public))
    path, _ = status()
    refused = tool(bins / 'dytallix-control', 'assemble', '--config', config, '--status', path,
                   '--request', request_path, '--out', drill / 'freeze-old-control.json', *old, check=False)
    if refused.returncode == 0 or 'not in this control' not in refused.stderr:
        raise Failed(f'assembly took the old kit\'s key: {refused.stderr.strip()}')
    # The node decodes only its canonical form: the new kit's control as
    # assembly writes it, with the new key's signature swapped for the old
    # kit's, so that the node refuses it for the key and nothing else.
    control = assemble(request_path, request, old[:2] + [sign(request_path, request, *new_key('freeze'))])
    forced = with_signature(control.read_bytes(), arriving_id, json.loads(old[2].read_bytes()))
    (drill / 'freeze-old-control.json').write_bytes(forced)
    checked = json.loads(tool(bins / 'dytallix', 'control', 'check', drill / 'freeze-old-control.json',
                              env=env, quiet=True).stdout)
    if checked['status'] != 'REFUSED' or 'Emergency authority key missing' not in checked['log']:
        raise Failed(f'the node did not refuse the old kit\'s key: {checked}')
    say(f'== the node refuses the old kit: {checked["log"]}')
    # The new kit freezes, then resumes, the chain.
    freeze = send(control, 'freeze')
    _, frozen = wait('the chain is frozen', lambda view: view['emergency_control']['frozen'])
    readiness = hashlib.sha256(b'kit replacement drill readiness').hexdigest()
    request_path, request = prepare('resume', '--incident-sha256', incident, '--readiness-sha256', readiness)
    signatures = [sign(request_path, request, *kit_key(n, 'resume')) for n in (3, 4)]
    resume = submit(request_path, request, signatures + [sign(request_path, request, *new_key('resume'))])
    _, resumed = wait('the chain resumes', lambda view: not view['emergency_control']['frozen'])

    leaving = {purpose: json.loads(kit_key(SEAT, purpose)[1].read_bytes())['key_id']
               for purpose in ('upgrade', 'freeze', 'resume')}
    arriving = {purpose: json.loads(new_key(purpose)[1].read_bytes())['key_id']
                for purpose in ('upgrade', 'freeze', 'resume')}
    say(json.dumps({'kit_drill': {
        'chain_id': staging_host.CHAIN_ID, 'seat': SEAT, 'leaving_key_ids': leaving, 'new_key_ids': arriving,
        'replacement_control_sha256': replacement, 'admitted_by_height': admitted['height'],
        'effect_height': effect, 'epoch_2_seen_at_height': switched['height'],
        'old_kit_signer_refused': signer_refused, 'old_kit_refused': checked['log'],
        'freeze_control_sha256': freeze, 'frozen_at_height': frozen['height'],
        'resume_control_sha256': resume, 'resumed_at_height': resumed['height']}}, indent=2))
    say(f'== seat {SEAT}\'s kit was replaced: after the effect height the old kit is refused, '
        'and the new kit froze and resumed the chain')


# The monitoring drill (monitoring v1, M5)

ALERTS_PORT = 9100
ALERTS = '/var/tmp/alerts'
EVENTS = f'{ALERTS}/events.jsonl'
# The alerting service's approved rule: a heartbeat missing for 3 minutes.
MONITORING_DOWN_SECONDS = 180
# A file on the data disk's file system that leaves it under the critical level.
FILL = '/var/lib/dytallix-drill-fill'
FILL_FREE_PERCENT = 3


# Every drill write: a basic transaction uses about 47,600 gas and pays the
# 1 DRT floor; the cap bounds what a write may be charged.
WALLET_WRITE = ('--gas-limit', '200000', '--maximum-fee-udrt', '20000000')
DGT_TOTAL = 10**15


def supply_holds(view):
    """The supply rule in a status view: DRT total is genesis plus emitted
    minus burned, and every DGT is issued and none minted."""
    drt, dgt = view['supply'], view['dgt_supply']
    return (int(drt['total']) == int(drt['genesis']) + int(drt['emitted']) - int(drt['burned'])
            and int(dgt['issued']) == DGT_TOTAL)


def pinned_cli(work, directory):
    """A CLI home with the rehearsal wallets, pinned to the public node's
    client channel and the staging genesis: the environment to run it in."""
    work, directory = Path(work), Path(directory)
    chain = work / 'net' / 'chain'
    wallets = chain / 'rehearsal-wallets'
    directory.mkdir(parents=True)
    shutil.copytree(wallets / 'home', directory / 'home')
    env = dict(os.environ, HOME=str(directory / 'home'), DYTALLIX_KEYSTORE_PASSPHRASE_FILE=str(wallets / 'passphrase'))
    digest = hashlib.sha256((chain / 'chain' / 'native-genesis.json').read_bytes()).hexdigest()
    tool(work / 'release-bin' / 'dytallix', 'config', 'pin-chain', '--endpoint',
         work / 'net' / 'keys' / f'{PUBLIC}.channel-pin.json', '--network', 'testnet', '--chain-id',
         staging_host.CHAIN_ID, '--genesis-digest', digest, env=env)
    return env


def wallet_drill(work, timeout=300):
    """The rehearsal wallets on the staging chain (simulation plan, T1): over
    the public node's client channel, with the release's CLI, a DRT transfer
    that creates an account, a DGT transfer, a bond to the validator and a
    reward claim, each by its committed receipt and proven balances, with the
    supply rule before and after. It keeps the ledger of its writes, from the
    status view saved before the first (status-1.json), for the
    reconciliation (T6)."""
    work = Path(work)
    bins, chain = work / 'release-bin', work / 'net' / 'chain'
    wallets = chain / 'rehearsal-wallets'
    accounts = {a['label']: a for a in json.loads((wallets / 'MAPPING.json').read_bytes())['accounts']}
    drill = work / 'wallet-drill'
    env = pinned_cli(work, drill)
    saved = iter(range(1, 1_000_000))
    ledger = []

    def cli(*args):
        return json.loads(tool(bins / 'dytallix', *args, env=env, quiet=True).stdout)

    def status():
        path = drill / f'status-{next(saved)}.json'
        tool(bins / 'dytallix', 'control', 'status', '--out', path, env=env, quiet=True)
        return json.loads(path.read_bytes())

    def write(*args, created=0):
        report = cli(*args, *WALLET_WRITE)
        if not (report.get('committed') and report.get('status') == 'success'):
            raise Failed(f'{args[0]} did not commit successfully: {report.get("status")}')
        say(f'== {" ".join(args[:2])}: committed, charged {report["charge_udrt"]} udrt')
        ledger.append({'what': ' '.join(args[:2]), 'charge_udrt': str(report['charge_udrt']),
                       'created_accounts': created})
        return report

    def balance(target):
        flag = ('--wallet', target) if target in accounts or target == 'drill-new' else (target,)
        return cli('balance', *flag)

    before = status()
    if not supply_holds(before):
        raise Failed(f'the supply rule fails before the drill at height {before["height"]}')

    # A transfer to an address with no account creates it (account model v2).
    tool(bins / 'dytallix', 'wallet', 'create', '--name', 'drill-new', env=env, quiet=True)
    fresh = balance('drill-new')
    if fresh['account_exists']:
        raise Failed('the new wallet already has an account')
    write('send', '--to', fresh['address'], '--amount', '100', '--token', 'drt', '--wallet', 'ecosystem-growth',
          created=1)
    created = balance('drill-new')
    if not created['account_exists'] or created['udrt'] != str(100 * 10**6):
        raise Failed(f'the new account holds {created["udrt"]} udrt, not 100 DRT')

    holder = accounts['kit-holder-1']
    start = balance('kit-holder-1')
    write('send', '--to', holder['address'], '--amount', '1000', '--token', 'dgt', '--wallet', 'ecosystem-growth')
    if int(balance('kit-holder-1')['udgt']) != int(start['udgt']) + 1000 * 10**6:
        raise Failed('kit-holder-1 did not receive 1,000 DGT')

    validator = next(v['validator_id'] for v in json.loads((chain / 'genesis' / 'records.json').read_bytes())['validators'])
    write('stake', 'bond', '--validator', validator, '--amount', '1000', '--wallet', 'kit-holder-1')

    # A bond joins the set at H+2; then it earns its share of each block's
    # staking budget, which a claim pays.
    deadline = time.monotonic() + timeout
    while True:
        summary = cli('stake', 'status', '--wallet', 'kit-holder-1')
        if int(summary['bonded_udgt']) >= 1000 * 10**6 and int(summary['claimable_rewards_udrt']) > 0:
            break
        if time.monotonic() > deadline:
            raise Failed(f'kit-holder-1 has bonded {summary["bonded_udgt"]} udgt and '
                         f'{summary["claimable_rewards_udrt"]} udrt claimable')
        time.sleep(5)
    liquid = int(summary['liquid_udrt'])
    claim = write('stake', 'claim', '--wallet', 'kit-holder-1')
    claimed = cli('stake', 'status', '--wallet', 'kit-holder-1')
    if int(claimed['liquid_udrt']) <= liquid - int(claim['charge_udrt']):
        raise Failed('the reward claim paid nothing')

    after = status()
    if not supply_holds(after):
        raise Failed(f'the supply rule fails after the drill at height {after["height"]}')
    burned = int(after['supply']['burned']) - int(before['supply']['burned'])
    if burned < 10 * 10**6:
        raise Failed(f'only {burned} udrt was burned; the account creation fee alone is 10 DRT')
    evidence = {'from_height': before['height'], 'to_height': after['height'], 'burned_udrt': str(burned),
                'emitted_udrt': str(int(after['supply']['emitted']) - int(before['supply']['emitted'])),
                'new_account': created['address'], 'supply_rule': 'holds'}
    (drill / 'evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
    fee = json.loads((chain / 'chain' / 'application-config.json').read_bytes())['ordinary']['fee_profile']
    (drill / 'ledger.json').write_text(json.dumps({
        'schema': reconcile.LEDGER_SCHEMA, 'from_height': before['height'],
        'account_creation_fee_udrt': str(fee['account_creation_fee_udrt']), 'entries': ledger}, indent=2) + '\n')
    say(json.dumps(evidence, indent=2))


# The reconciliation report (simulation plan, T6)

def reconcile_network(work, after='wallet-drill', timeout=300):
    """The reconciliation of the staging network after the wallet drill, or
    after the load step: the validator on the runner and the sentry through
    its guest agent, the client over the sentry's client channel, every
    rehearsal account, the drill's new one and (after the load) every account
    the load created tracked, the burns since the step's first status view against
    its ledger, and the sentry's database checked on a stopped copy.
    Nothing transacts between the step and this one."""
    work = Path(work)
    chain, drill = work / 'net' / 'chain', work / 'wallet-drill'
    mapping = json.loads((chain / 'rehearsal-wallets' / 'MAPPING.json').read_bytes())
    addresses = [a['address'] for a in mapping['accounts']]
    addresses.append(json.loads((drill / 'evidence.json').read_bytes())['new_account'])
    if after == 'load':
        out, run = work / 'reconcile-load', work / 'load' / 'run'
        addresses += json.loads((run / 'accounts.json').read_bytes())['addresses']
        since, ledger = run / 'status-before.json', run / 'ledger.json'
    else:
        out, since, ledger = work / 'reconcile', drill / 'status-1.json', drill / 'ledger.json'
    env = pinned_cli(work, out)
    client = reconcile.Client(work / 'release-bin' / 'dytallix', env, out)
    hosts = {'validator-1': reconcile.LocalHost(), **{label: agent(work, label) for label in VMS}}
    record = reconcile.collect(
        staging_host.CHAIN_ID, hosts, (chain / 'chain' / 'native-genesis.json').read_bytes(), client=client,
        addresses=addresses, since=json.loads(since.read_bytes()), ledger=json.loads(ledger.read_bytes()),
        state_check_host=PUBLIC, timeout=timeout, say=say)
    path = out / 'evidence.json'
    path.write_text(json.dumps(record, indent=2) + '\n')
    for row in record['checks']:
        say(f'{row["status"]:<12} {row["check"]}: {row["detail"]}')
    say(f'== reconciliation at height {record["height"]}: {record["status"]} (evidence sha256 {record["sha256"]})')
    if record['status'] != reconcile.PASS:
        raise Failed(f'the reconciliation is {record["status"]}')


# The load generator (simulation plan, T3)

# A short, low-rate run for one CI runner: one address, so two channel
# connections. The capacity runs (T05) use load.py from the load machines.
LOAD_CI = {'wallets': 16, 'voters': 1, 'spares': 8, 'distributors': 2, 'rate': 1.0, 'duration': 120,
           'sweep_interval': 60, 'sweep_from': ('operator-1', 'kit-holder-1')}


def load_drill(work):
    """The load generator on the staging network: the fleet made with the
    release's CLI, funded from the ecosystem growth account through two
    distributors and primed, then a short paced run of the default mix over
    the sentry's client channel with the governance steps beside it, and
    one sweep of the operator's and kit-holder-1's rewards back to the
    funder. Every transaction must be committed or
    refused at admission (none may be refused here), 99% within 2 blocks,
    and the burns must match the run's ledger. The evidence's digests are
    checked again from its files."""
    work = Path(work)
    chain = work / 'net' / 'chain'
    out = work / 'load'
    env = pinned_cli(work, out)
    cli = work / 'release-bin' / 'dytallix'
    load_generator.make_fleet(cli, staging_host.CHAIN_ID, out / 'fleet', LOAD_CI['wallets'], LOAD_CI['voters'],
                              LOAD_CI['spares'], LOAD_CI['distributors'])
    validator = next(v['validator_id'] for v in json.loads((chain / 'genesis' / 'records.json').read_bytes())['validators'])
    settings = load_generator.Settings(rate=LOAD_CI['rate'], duration=LOAD_CI['duration'], governance=True,
                                       max_refused=0, sweep_interval=LOAD_CI['sweep_interval'])
    record = load_generator.run_load(cli, env['HOME'], out / 'fleet', out / 'run', chain / 'chain' / 'application-config.json',
                                     'ecosystem-growth', validator, settings, show=say,
                                     passphrase=env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'],
                                     sweep_from=LOAD_CI['sweep_from'])
    load_generator.report(record, show=say)
    checked = load_generator.verify(json.loads((out / 'run' / 'evidence.json').read_bytes()),
                                    (out / 'run' / 'transactions.jsonl').read_bytes())
    if record['status'] != load_generator.PASS or checked != record['status']:
        raise Failed(f'the load run is {record["status"]} (verified {checked})')


def checkpoint_network(work, timeout=300):
    """The checkpoint recorder (simulation plan, T7) on the staging network: a
    short schedule of the R07 checks CI can source, recorded against the fresh
    after-load reconciliation (T6) and the load run (T3), with the client
    channel read for RPC and wallet connectivity and the genesis digest
    confirmed. The reconciliation and load evidence are bound by digest and
    re-verified; the record verifies to PASS, and the same record is refused as
    the R07 dress rehearsal, since this schedule is not R07's. Run after the
    after-load reconciliation so its evidence is fresh, not the wallet drill's."""
    work = Path(work)
    chain = work / 'net' / 'chain'
    native = (chain / 'chain' / 'native-genesis.json').read_bytes()
    engine = (chain / 'chain' / 'genesis.json').read_bytes()
    out = work / 'checkpoint'
    # The client channel's CLI home; pinned_cli creates the directory (as the wallet drill's).
    env = pinned_cli(work, out)
    # Bind the fresh after-load reconciliation and the load run, kept beside the record.
    shutil.copyfile(work / 'reconcile-load' / 'evidence.json', out / 'reconcile.json')
    shutil.copyfile(work / 'load' / 'run' / 'evidence.json', out / 'load-evidence.json')
    shutil.copyfile(work / 'load' / 'run' / 'transactions.jsonl', out / 'load-transactions.jsonl')
    reconcile_bind = {'tool': 'reconcile', 'evidence': 'reconcile.json',
                      'evidence_sha256': checkpoints.digest((out / 'reconcile.json').read_bytes())}
    load_bind = {'tool': 'load', 'evidence': 'load-evidence.json',
                 'evidence_sha256': checkpoints.digest((out / 'load-evidence.json').read_bytes()),
                 'rows': 'load-transactions.jsonl',
                 'rows_sha256': checkpoints.digest((out / 'load-transactions.jsonl').read_bytes())}
    # The client channel: the sentry's height, and a wallet balance it can read.
    client = reconcile.Client(work / 'release-bin' / 'dytallix', env, out)
    view = client.status()
    address = json.loads((chain / 'rehearsal-wallets' / 'MAPPING.json').read_bytes())['accounts'][0]['address']
    client.balance(address)
    readings = {
        'Verify token supply.': {'bind': reconcile_bind},
        'Verify state consistency across validators.': {'bind': reconcile_bind},
        'Confirm block production.': {'bind': load_bind},
        'Verify finalization.': {'bind': load_bind},
        'Verify RPC.': {'inputs': {'reachable': True, 'height': int(view['height'])}},
        'Verify wallet connectivity.': {'inputs': {'reachable': True}},
    }
    # One checkpoint, its window opening when the load started, so the fresh
    # after-load reconciliation and the load run fall in it and the stale
    # wallet-drill reconciliation (finished earlier) would not.
    load_started = json.loads((out / 'load-evidence.json').read_bytes())['started']
    genesis_time = json.loads(engine)['genesis_time']
    offset = int((checkpoints.instant(load_started) - checkpoints.instant(genesis_time)).total_seconds())
    schedule = [{'label': 'T ZERO', 'offset_seconds': offset, 'checks': list(readings)}]
    finished = json.loads((out / 'reconcile.json').read_bytes())['finished']
    checkpoint = checkpoints.record_checkpoint(schedule, 0, readings, out, native, engine, '', finished)
    record = checkpoints.build_run('staging-checkpoint', schedule, staging_host.CHAIN_ID, native, engine, [checkpoint])
    (out / 'evidence.json').write_text(json.dumps(record, indent=2) + '\n')
    for row in checkpoint['checks']:
        say(f'{row["status"]:<12} {row["check"]}: {row["detail"]}')
    status = checkpoints.verify(record, out, native, engine)
    say(f'== checkpoint {checkpoint["label"]}: {status} (evidence sha256 {record["sha256"]})')
    if status != checkpoints.PASS:
        raise Failed(f'the checkpoint record is {status}')
    try:
        checkpoints.verify(record, out, native, engine, qualifying='R07')
    except checkpoints.Failed:
        say('== a short schedule is refused as the R07 dress rehearsal, as it must be')
    else:
        raise Failed('a non-R07 schedule was accepted as the dress rehearsal')
    # The window guard: the stale wallet-drill reconciliation (finished before
    # the load started) does not count for this checkpoint.
    shutil.copyfile(work / 'reconcile' / 'evidence.json', out / 'reconcile-stale.json')
    stale = {'tool': 'reconcile', 'evidence': 'reconcile-stale.json',
             'evidence_sha256': checkpoints.digest((out / 'reconcile-stale.json').read_bytes())}
    stale_cp = checkpoints.record_checkpoint(schedule, 0, {'Verify token supply.': {'bind': stale}},
                                             out, native, engine, '', finished)
    stale_status = next(r['status'] for r in stale_cp['checks'] if r['check'] == 'Verify token supply.')
    if stale_status != checkpoints.INCONCLUSIVE:
        raise Failed(f'the stale reconciliation counted for the checkpoint ({stale_status})')
    say('== the stale wallet-drill reconciliation does not count for the checkpoint, as it must not')


def parse_events(raw):
    """The stand-in service's events; a line still being written is skipped."""
    events = []
    for line in raw.splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            continue
    return events


def alert_events(guest):
    code, raw, _ = guest.run(['cat', EVENTS], check=False)
    return parse_events(raw) if code == 0 else []


def find_event(events, condition, status, after):
    """The first alert for the condition with the status from event index
    `after` on, as (index, event), or None."""
    for index, event in enumerate(events[after:], start=after):
        body = event['body']
        if event['to'] in ('alert', 'service') and body.get('condition') == condition and body.get('status') == status:
            return index, event
    return None


def guest_time(guest):
    _, out, _ = guest.run(['date', '+%s'])
    return int(out.strip())


def installed_thresholds(guest):
    """The thresholds in the monitor configuration the host's unit runs."""
    _, unit, _ = guest.run(['systemctl', 'cat', 'dytallix-monitor.service'])
    found = re.search(r'--config (\S+)', unit)
    if not found:
        raise Failed('the monitor unit names no configuration')
    return json.loads(guest.read(found.group(1)))['thresholds']


def fill_size(stat_line, free_percent):
    """Bytes to allocate so that the unprivileged free space (statvfs
    f_bavail, what the monitor reads) is free_percent of the disk, from
    `stat -f -c '%a %b %S'` (available blocks, total blocks, block size)."""
    available, total, size = (int(x) for x in stat_line.split())
    return max(available - total * free_percent // 100, 0) * size


def monitor_drill(work, label='sentry-1'):
    """A halt, a restart loop, a full disk and a stopped monitor on the
    sentry, with the approved rules: each must reach the stand-in alerting
    service as an alert and then resolve. It prints the evidence."""
    work = Path(work)
    guest = agent(work, label)
    say(f'== a stand-in alerting service on {label}\'s loopback, and its monitor settings')
    guest.run(['rm', '-rf', ALERTS])
    guest.run(['systemd-run', '--unit=dytallix-alert-standin', 'python3', '/root/alert_standin.py', '--root', ALERTS,
               '--port', str(ALERTS_PORT), '--missing-seconds', str(MONITORING_DOWN_SECONDS)])
    time.sleep(2)
    base = f'http://127.0.0.1:{ALERTS_PORT}'
    settings = {'schema': 'dytallix.monitor-settings.v1', 'label': label, 'heartbeat_url': f'{base}/heartbeat',
                'alert_url': f'{base}/alert', 'escalation_url': f'{base}/escalation'}
    guest.run(['sh', '-c', 'umask 077 && cat > /root/monitor-settings.json'], stdin=json.dumps(settings))
    _, out, _ = guest.run(['/root/node/dytallix-host/monitor-settings.sh', '/root/monitor-settings.json'], timeout=180)
    say(out.strip())

    def heartbeat_after(moment, timeout=180):
        """The first heartbeat from a run that started after the moment
        (the guest's clock), as (index, payload)."""
        deadline = time.monotonic() + timeout
        while True:
            for index, event in enumerate(alert_events(guest)):
                if event['to'] == 'heartbeat' and event['body']['time'] > moment:
                    return index, event['body']
            if time.monotonic() > deadline:
                raise Failed(f'{label}: no heartbeat from a run after {moment} in {timeout} s')
            time.sleep(5)

    def wait_event(condition, status, after, timeout):
        deadline = time.monotonic() + timeout
        while True:
            found = find_event(alert_events(guest), condition, status, after)
            if found:
                index, event = found
                say(f'== {condition} {status} at {event["received"]}: value {json.dumps(event["body"]["value"])}, '
                    f'threshold {event["body"]["threshold"]}')
                return index, event
            if time.monotonic() > deadline:
                raise Failed(f'{label}: no {condition} {status} in {timeout} s')
            time.sleep(5)

    index, first = heartbeat_after(guest_time(guest) - 1)
    if first['firing']:
        raise Failed(f'{label} is already firing {first["firing"]} before the drill')
    say(f'== heartbeat received; nothing firing at height {first["height"]}')
    evidence = {}

    def scenario(name, conditions, start, cause, cure, timeout):
        """Starts the cause, waits for each condition to fire, removes the
        cause and waits for each to resolve."""
        say(f'== {name}')
        mark = len(alert_events(guest))
        cause()
        fired = {c: wait_event(c, 'firing', mark, timeout) for c in conditions}
        cure()
        resolved = {c: wait_event(c, 'resolved', fired[c][0] + 1, timeout) for c in conditions}
        evidence[name] = {c: {'fired_at': fired[c][1]['received'], 'fired_value': fired[c][1]['body']['value'],
                              'threshold': fired[c][1]['body']['threshold'],
                              'resolved_at': resolved[c][1]['received'], 'to': fired[c][1]['to']}
                          for c in conditions}
        evidence[name]['started_at'] = start

    # Restart loop: three new starts of the node within the window, each seen
    # by a monitor run. It resolves once the first leaves the window
    # (15 minutes), so it is judged last; the other drills run meanwhile.
    say('== restart loop: the node is restarted three times')
    mark, starts = len(alert_events(guest)), []
    for _ in range(3):
        guest.run(['systemctl', 'restart', 'dytallix-node'], timeout=180)
        starts.append(guest_time(guest))
        heartbeat_after(starts[-1])
    loop_index, loop_fired = wait_event('restart_loop', 'firing', mark, 180)

    def fill():
        critical = installed_thresholds(guest)['disk_free_critical_percent']
        if FILL_FREE_PERCENT >= critical:
            raise Failed(f'the drill leaves {FILL_FREE_PERCENT}% free, not under the critical {critical}%')
        _, line, _ = guest.run(['stat', '-f', '-c', '%a %b %S', '/var/lib/dytallix'])
        size = fill_size(line, FILL_FREE_PERCENT)
        _, same, _ = guest.run(['sh', '-c', f'touch {FILL} && test "$(stat -c %d {FILL})" = '
                                            '"$(stat -c %d /var/lib/dytallix)" && echo same'])
        if same.strip() != 'same':
            raise Failed(f'{FILL} is not on the data disk')
        guest.run(['fallocate', '-l', str(size), FILL])
        _, df, _ = guest.run(['df', '-h', '/var/lib/dytallix'])
        say(df.strip())

    scenario('full_disk', ('disk_low', 'disk_critical'), guest_time(guest), fill,
             lambda: guest.run(['rm', '-f', FILL]), 180)

    # A halt: the validator, on the runner, stops; the sentry's height stops.
    def halt():
        subprocess.run(['systemctl', 'stop', f'{staging_host.UNIT}.service'], check=True)

    def restart_validator():
        subprocess.run(['systemctl', 'start', f'{staging_host.UNIT}.service'], check=True)
        staging_host.wait_for((staging_host.height() or 0) + 2, 300)

    scenario('halt', ('consensus_halt',), guest_time(guest), halt, restart_validator, 300)

    # A stopped monitor: only the alerting service can see it.
    def stop_monitor():
        guest.run(['systemctl', 'stop', 'dytallix-monitor.timer', 'dytallix-monitor.service'], timeout=180)

    scenario('stopped_monitor', ('monitoring_down',), guest_time(guest), stop_monitor,
             lambda: guest.run(['systemctl', 'start', 'dytallix-monitor.timer']), MONITORING_DOWN_SECONDS + 180)

    window = installed_thresholds(guest)['restarts_window_seconds']
    say(f'== restart loop: waiting for the first start to leave the {window} s window')
    _, loop_resolved = wait_event('restart_loop', 'resolved', loop_index + 1,
                                  max(starts[0] + window - guest_time(guest), 0) + 240)
    evidence['restart_loop'] = {'restart_loop': {
        'fired_at': loop_fired['received'], 'fired_value': loop_fired['body']['value'],
        'threshold': loop_fired['body']['threshold'], 'resolved_at': loop_resolved['received'], 'to': 'alert'},
        'started_at': starts[0], 'starts': starts}

    _, last = heartbeat_after(guest_time(guest))
    if last['firing']:
        raise Failed(f'{label} is still firing {last["firing"]} after the drill')
    events = alert_events(guest)
    seen = sorted({(e['body']['condition'], e['body']['status']) for e in events if e['to'] in ('alert', 'service')})
    escalations = [e['body']['condition'] for e in events if e['to'] == 'escalation']
    say(json.dumps({'monitor_drill': {
        'chain_id': staging_host.CHAIN_ID, 'host': label, 'scenarios': evidence,
        'alerts_seen': [f'{c} {s}' for c, s in seen], 'escalations': escalations,
        'heartbeats': sum(e['to'] == 'heartbeat' for e in events),
        'final_heartbeat': {k: last[k] for k in ('time', 'height', 'firing', 'undelivered', 'delivery_failures')}}},
        indent=2))
    guest.run(['systemctl', 'stop', 'dytallix-alert-standin'], check=False)
    say(f'== {label}: a restart loop, a full disk, a halt and a stopped monitor each alerted and resolved')


# The staging agent (simulation plan, T2)

COMMAND_STORE = dict(UPLOAD, prefix='staging-commands/', access_key_id='STANDINREAD', secret_access_key='cmVhZC1vbmx5')
# The founder's own key (T2's real-host harness): PUT, GET and LIST, over the
# network as the founder reaches the real store.
FOUNDER_STORE = {'schema': staging_founder.STORE_SCHEMA, 'endpoint': f'http://{ADDRESSES["sentry-1"]}:{STANDIN_PORT}',
                 'bucket': COMMAND_STORE['bucket'], 'region': COMMAND_STORE['region'],
                 'access_key_id': 'STANDINFOUNDER', 'secret_access_key': 'Zm91bmRlci1vbmx5',
                 'commands_prefix': COMMAND_STORE['prefix'], 'results_prefix': HISTORY_UPLOAD['prefix']}


def put_file(guest, path, text, mode='0644'):
    guest.run(['sh', '-c', f'mkdir -p "$(dirname {path})" && umask 077 && cat > {path} && chmod {mode} {path}'], stdin=text)


def founder_channel(work, guest, label, drill, unit):
    """The founder's side of the agent (T2), as on a real host: the staging
    command key made with the release's CLI, the agent installed beside the
    node with a read key for its own command prefix, and a channel that
    reaches the store only through its S3 API (staging_founder.py) with the
    founder's key. The stand-in store on the guest stands for the real one:
    the agent and the host's jobs use it on loopback, the founder over the
    network, which this CI-only rule opens to the runner alone."""
    passphrase = drill / 'passphrase'
    descriptor = os.open(passphrase, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        file.write(hashlib.sha256(os.urandom(32)).hexdigest() + '\n')
    os.environ['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    cli, home, key = work / 'release-bin' / 'dytallix', drill / 'founder', drill / 'key.json'
    staging_command.make_key(cli, home, key)
    (drill / 'read.json').write_text(json.dumps(COMMAND_STORE))
    staging_command.make_settings(key, label, staging_host.CHAIN_ID, drill / 'read.json', drill / 'settings.json')
    guest.run(['systemctl', 'stop', 'dytallix-store-standin', 'dytallix-store-standin-agent',
               'dytallix-store-standin-fault'], check=False)
    guest.run(['rm', '-rf', f'/var/tmp/standin/{COMMAND_STORE["bucket"]}/{COMMAND_STORE["prefix"]}',
               f'/var/tmp/standin/{HISTORY_UPLOAD["bucket"]}/{HISTORY_UPLOAD["prefix"]}{staging_host.CHAIN_ID}/agent'],
              check=False)
    guest.run(['systemd-run', f'--unit={unit}', 'python3', '/root/s3_standin.py', '--root', '/var/tmp/standin',
               '--port', str(STANDIN_PORT), '--bind', '0.0.0.0', '--access-key-id', UPLOAD['access_key_id'],
               '--access-key-id', HISTORY_UPLOAD['access_key_id'], '--read-key-id', COMMAND_STORE['access_key_id'],
               '--founder-key-id', FOUNDER_STORE['access_key_id']])
    # The host firewall drops what it does not list; let the runner reach the
    # stand-in (absent the table, the port is open anyway).
    guest.run(['nft', 'insert', 'rule', 'inet', 'dytallix_node', 'input', 'ip', 'saddr', ADDRESSES['validator-1'],
               'tcp', 'dport', str(STANDIN_PORT), 'accept'], check=False)
    wait_standin(guest, unit)
    for name in ('agent.py', 'ALLOWLIST.json'):
        put_file(guest, f'/root/agent/{name}', (staging_command.AGENT_DIR / name).read_text())
    put_file(guest, '/root/agent-settings.json', (drill / 'settings.json').read_text(), '0600')
    _, out, _ = guest.run(['python3', '-I', '-B', '/root/agent/agent.py', 'install', '--settings', '/root/agent-settings.json'])
    say(out.strip())
    codes = json.loads((work / 'net' / 'seal-codes.json').read_bytes())
    paper = drill / 'history-paper.txt'
    descriptor = os.open(paper, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        file.write(codes['history'] + '\n')

    def poll_now(seconds):
        """CI only: run the agent's poll now instead of waiting for its
        one-minute timer. The founder never can; the channel just waits."""
        guest.run(['systemctl', 'start', 'dytallix-agent.service'], timeout=600, check=False)
        time.sleep(min(seconds, 3))

    store = staging_founder.Store(staging_founder.check_store(FOUNDER_STORE, standin=True), drill / 'store-scratch')
    return staging_founder.Channel(store, staging_host.CHAIN_ID, label, drill / 'results', cli, home, key, paper,
                                   work / 'release-bin' / 'dytallix-root-sign', sleep=poll_now, say=say)


def wait_standin(guest, unit, timeout=60):
    """The stand-in answers on the guest's loopback (where the agent reaches it)
    and from the runner (where the founder's side does)."""
    deadline = time.monotonic() + timeout
    probe = 'import socket, sys; socket.create_connection((sys.argv[1], int(sys.argv[2])), 3)'
    while True:
        code, _, _ = guest.run(['python3', '-c', probe, '127.0.0.1', str(STANDIN_PORT)], check=False)
        if code == 0:
            break
        if time.monotonic() > deadline:
            _, state, _ = guest.run(['systemctl', 'status', '--no-pager', unit], check=False)
            _, journal, _ = guest.run(['journalctl', '-u', unit, '-o', 'cat', '--no-pager', '-n', '40'], check=False)
            raise Failed(f'the stand-in store ({unit}) never listened on port {STANDIN_PORT}:\n{state}\n{journal}')
        time.sleep(1)
    try:
        socket.create_connection((ADDRESSES[PUBLIC], STANDIN_PORT), 5).close()
    except OSError as error:
        _, rules, _ = guest.run(['nft', 'list', 'ruleset'], check=False)
        raise Failed(f'the runner cannot reach the stand-in at {ADDRESSES[PUBLIC]}:{STANDIN_PORT} ({error}); '
                     f'the guest ruleset:\n{rules}')


def close_founder_channel(guest, unit):
    guest.run(['systemctl', 'stop', unit], check=False)
    guest.run(['nft', 'delete', 'rule', 'inet', 'dytallix_node', 'input', 'handle',
               *founder_rule_handle(guest)], check=False)


def founder_rule_handle(guest):
    _, listing, _ = guest.run(['nft', '-a', 'list', 'chain', 'inet', 'dytallix_node', 'input'], check=False)
    found = re.search(rf'ip saddr {re.escape(ADDRESSES["validator-1"])} tcp dport {STANDIN_PORT} accept # handle (\d+)',
                      listing)
    return [found.group(1)] if found else []


def agent_drill(work, label='sentry-1', timeout=300):
    """The staging agent (T2) on the sentry, driven only through the founder's
    path (staging_founder.py): signed commands PUT into the store with the
    founder's key, sealed results LISTed, fetched and opened with the history
    code. A signed status and a signed journal run; a tampered, an unsigned, a
    replayed, a non-allowlisted and an expired command are refused. Then
    wipe.sh removes the node and the agent together."""
    work = Path(work)
    guest, chain, cli = agent(work, label), staging_host.CHAIN_ID, work / 'release-bin' / 'dytallix'
    drill = work / 'agent-drill'
    drill.mkdir()
    unit = 'dytallix-store-standin-agent'
    channel = founder_channel(work, guest, label, drill, unit)
    now = guest_time(guest)

    def make(sequence, command, args=None, not_after=None):
        body = staging_command.body(chain, label, sequence, command, args or {}, not_after or now + 1800)
        return staging_command.sign(cli, channel.home, channel.key, body)

    status = make(1, 'status')
    tampered = json.loads(make(2, 'status'))
    tampered['body']['command'] = 'stop'
    unsigned = {'body': staging_command.body(chain, label, 3, 'stop', {}, now + 1800)}
    commands = {1: status, 2: json.dumps(tampered).encode(), 3: json.dumps(unsigned).encode(), 4: status,
                5: make(5, 'shell'), 6: make(6, 'stop', not_after=now - 60), 7: make(7, 'journal', {'lines': 20})}
    expected = {1: ('DONE', None), 2: ('REFUSED', 'does not verify'), 3: ('REFUSED', 'not a signed command'),
                4: ('REFUSED', 'signed as sequence 1, expected 4'), 5: ('REFUSED', 'not on the allowlist'),
                6: ('REFUSED', 'expired'), 7: ('DONE', None)}
    # The hostile objects go in through the same store API a planted object would.
    for sequence, raw in commands.items():
        channel.store.put(f'{channel.commands}{sequence:010}.json', raw)
    if channel.next_sequence() != 8:
        raise Failed('the founder\'s listing does not show the seven commands')
    evidence = {r['sequence']: r for r in channel.wait(list(commands), timeout)}
    for sequence, (status_expected, reason) in expected.items():
        result = evidence.get(sequence)
        if result is None or result['status'] != status_expected or (reason and reason not in (result['reason'] or '')):
            raise Failed(f'command {sequence}: expected {status_expected} {reason or ""}, got {result}')
    if evidence[1]['steps'][0]['stdout'].strip() != 'active' or not evidence[7]['steps'][0]['stdout'].strip():
        raise Failed(f'the status and journal results hold no output: {evidence[1]["steps"][0]}')
    # The founder's own next command takes the next free slot and runs.
    last = channel.run('status', timeout=timeout)
    if last['sequence'] != 8 or last['status'] != 'DONE':
        raise Failed(f'the founder\'s next command did not run at sequence 8: {last}')
    agent_reconcile(work, guest, label, channel, timeout)
    # Nothing refused ran: the node never stopped.
    _, active, _ = guest.run(['systemctl', 'is-active', 'dytallix-node'], check=False)
    if active.strip() != 'active':
        raise Failed(f'{label}\'s node is {active.strip()} after the refused stop commands')
    wait_height(guest, label, (guest_height(guest) or 0) + 2, timeout)
    say(json.dumps({'agent_drill': {str(n): {'status': r['status'], 'command': r['command'], 'reason': r['reason']}
                                    for n, r in sorted(channel.opened.items())}}, indent=2))
    close_founder_channel(guest, unit)
    # wipe.sh removes the agent with the node.
    _, out, _ = guest.run(['/root/node/dytallix-host/wipe.sh'], stdin=f'wipe {label}\n', timeout=300)
    say(out.strip())
    _, left, _ = guest.run(['sh', '-c', 'for p in /opt/dytallix-agent /etc/dytallix-agent /var/lib/dytallix-agent '
                                        '/etc/systemd/system/dytallix-agent.service /etc/systemd/system/dytallix-agent.timer; '
                                        'do [ -e "$p" ] && echo "$p"; done; true'])
    if left.strip():
        raise Failed(f'the wipe left the staging agent behind: {left.split()}')
    say(f'== {label}: through the founder\'s store API, signed commands ran once, the rest were refused, '
        'results were listed and opened with the history code; the wipe removed the agent')


def agent_reconcile(work, guest, label, channel, timeout):
    """The reconciliation (T6) as on console-only hosts
    (reconcile.collect_through_agent): the sentry read only through its signed
    reconcile-read, the validator on the runner meanwhile and the client over
    the sentry's client channel; no state check (it stays at the console).
    CI starts the guest's poll at once instead of on the minute."""
    out = work / 'reconcile-agent'
    env = pinned_cli(work, out)
    genesis = (work / 'net' / 'chain' / 'chain' / 'native-genesis.json').read_bytes()
    record = reconcile.collect_through_agent(
        staging_host.CHAIN_ID, {label: channel}, genesis, live={'validator-1': reconcile.LocalHost()},
        client=reconcile.Client(work / 'release-bin' / 'dytallix', env, out), align=False,
        after_send=lambda: guest.run(['systemctl', 'start', '--no-block', 'dytallix-agent.service']),
        timeout=timeout, say=say)
    path = out / 'evidence.json'
    path.write_text(json.dumps(record, indent=2) + '\n')
    for row in record['checks']:
        say(f'{row["status"]:<12} {row["check"]}: {row["detail"]}')
    bound = record['observations']['agent'][label]
    say(f'== reconciliation through {label}\'s agent at height {record["height"]}: {record["status"]} '
        f'(sequence {bound["sequence"]}, result {bound["result"]}; evidence sha256 {record["sha256"]})')
    if record['status'] != reconcile.PASS or reconcile.verify(json.loads(path.read_bytes()), genesis) != reconcile.PASS:
        raise Failed(f'the reconciliation through the agent is {record["status"]}')
    if not bound['result'] or bound['command_sha256'] != channel.sent[bound['sequence']]:
        raise Failed(f'the evidence does not bind the signed command and its sealed result: {bound}')


def adversary_drill(work):
    """The adversarial generator (simulation plan, T4) on the staging network:
    through the client channel, an invalid ML-DSA-65 signature, a decode failure,
    a stale nonce and an oversized request are each refused at admission, while a
    valid control transaction is admitted and committed. Writes digest-bound
    evidence that `adversary.py verify` re-evaluates. Duplicates, re-signed copies
    and floods follow with the load machines."""
    work = Path(work)
    chain = work / 'net' / 'chain'
    out = work / 'adversary'
    env = pinned_cli(work, out)
    cli = work / 'release-bin' / 'dytallix'
    load_generator.make_fleet(cli, staging_host.CHAIN_ID, out / 'fleet', 2, voters=0, spares=0, distributors=0)
    validator = next(v['validator_id'] for v in json.loads((chain / 'genesis' / 'records.json').read_bytes())['validators'])
    native = (chain / 'chain' / 'native-genesis.json').read_bytes()
    engine = (chain / 'chain' / 'genesis.json').read_bytes()
    record = load_generator.adversary_run(
        cli, env['HOME'], out / 'fleet', out / 'run', chain / 'chain' / 'application-config.json',
        'ecosystem-growth', validator, load_generator.Settings(), native, engine, PUBLIC, show=say,
        passphrase=env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'])
    for row in record['cases']:
        say(f'== {row["name"]}: {row["status"]} ({row["detail"]})')
    status = adversary.verify(json.loads((out / 'run' / 'evidence.json').read_bytes()), native, engine)
    say(json.dumps({'adversary_drill': {'host': PUBLIC, 'status': status,
                                        'cases': {r['name']: r['status'] for r in record['cases']}}}, indent=2))
    if status != adversary.PASS:
        raise Failed(f'the adversarial run is {status}')
    say(f'== {PUBLIC}: malformed traffic was refused at admission and the valid control committed (the run is {status})')


# The fault injector (simulation plan, T5)

def fault_gone(name, guest):
    """CI only: the host's own dead-man removed the fault (the netem qdisc or the
    partition element), which is what keeps a console-only host reachable."""
    if name in ('latency', 'loss'):
        _, qdisc, _ = guest.run(['tc', 'qdisc', 'show', 'dev', 'eth0'], check=False)
        return 'netem' not in qdisc
    _, blocked, _ = guest.run(['nft', 'list', 'set', 'inet', 'dytallix_fault', 'blocked'], check=False)
    return '26656' not in blocked


def fault_drill(work, label='sentry-1', timeout=300):
    """The fault injector (simulation plan, T5) on the sentry, run exactly as on
    a real host (faults.run_scenarios): each fault through the founder's channel
    to the signed agent, its proof from the command's read-only last step, and
    recovery timed on the public status page read from the runner about once a
    second (P01, 14 October 2026). The validator keeps producing throughout. It
    writes digest-bound evidence that `faults.py verify` re-evaluates, judged
    against the approved recovery target (launch/simulations/TIMINGS.json,
    fault_recovery_target_seconds). CI also checks, outside the evidence, that
    the host's own dead-man removed each network fault."""
    work = Path(work)
    guest = agent(work, label)
    drill = work / 'fault-drill'
    drill.mkdir()
    unit = 'dytallix-store-standin-fault'
    channel = founder_channel(work, guest, label, drill, unit)
    # A probe that exercises tc and nft first, so a missing sch_netem module, nft
    # support or interface fails in seconds, as the first command on a real host.
    probe = channel.run('fault-clear', timeout=timeout)
    if probe['status'] != 'DONE':
        raise Failed(f'the fault commands do not run on this host (tc/nft): {probe.get("steps")}')

    def dead_man(name):
        deadline = time.monotonic() + 60
        while not fault_gone(name, guest):
            if time.monotonic() > deadline:
                raise Failed(f'{name}: the host dead-man did not clear the fault')
            time.sleep(2)

    url = f'http://{ADDRESSES[PUBLIC]}:8080/status'
    rows = faults.run_scenarios(channel, lambda: staging_founder.status_page(url), staging_founder.block_instant,
                                result_timeout=timeout, recovery_timeout=timeout, say=say, on_cleared=dead_man)
    native = (work / 'net' / 'chain' / 'chain' / 'native-genesis.json').read_bytes()
    engine = (work / 'net' / 'chain' / 'chain' / 'genesis.json').read_bytes()
    timings = json.loads((Path(__file__).resolve().parents[3] / 'launch' / 'simulations' / 'TIMINGS.json').read_bytes())
    target = timings['fault_recovery_target_seconds']  # the approved T5 recovery target (P01, launch/approvals)
    record = faults.build_run(staging_host.CHAIN_ID, native, engine, rows, target, label)
    (drill / 'evidence.json').write_text(json.dumps(record, indent=2) + '\n')
    # Re-evaluate from the file, as the docs promise, not from the in-memory record.
    status = faults.verify(json.loads((drill / 'evidence.json').read_bytes()), native, engine)
    for row in record['scenarios']:
        say(f'== {row["name"]}: {row["status"]} ({row["detail"]})')
    say(json.dumps({'fault_drill': {'host': label, 'status': status,
                                    'scenarios': {r['name']: r['status'] for r in record['scenarios']}}}, indent=2))
    if status == faults.FAIL:
        raise Failed(f'the fault run is {status}')
    # Uninstall the agent (not the node) and close the store, so the agent drill
    # (T2) installs fresh.
    wait_height(guest, label, (guest_height(guest) or 0) + 2, timeout)
    _, left, _ = guest.run(['python3', '-I', '-B', '/root/agent/agent.py', 'remove'])
    say(left.strip())
    close_founder_channel(guest, unit)
    say(f'== {label}: latency, loss, a p2p partition and a process kill each took hold and the sentry caught back up '
        f'within the {target} s recovery target, measured through the public paths only (the run is {status})')


def diagnostics(work):
    work = Path(work)
    for label in VMS:
        say(f'===== {label}')
        console = work / 'vm' / f'{label}.console.log'
        if console.exists():
            say('\n'.join(console.read_text(errors='replace').splitlines()[-40:]))
        try:
            guest = agent(work, label)
            for args in (['systemctl', 'status', '--no-pager', 'dytallix-node'],
                         ['journalctl', '-u', 'dytallix-node', '-o', 'cat', '--no-pager', '-n', '200'],
                         ['journalctl', '-u', 'dytallix-backup', '-o', 'cat', '--no-pager', '-n', '50'],
                         ['journalctl', '-u', 'dytallix-monitor', '-o', 'cat', '--no-pager', '-n', '20'],
                         ['journalctl', '-u', 'dytallix-alert-standin', '-o', 'cat', '--no-pager', '-n', '20'],
                         ['journalctl', '-u', 'dytallix-agent', '-o', 'cat', '--no-pager', '-n', '20'],
                         ['journalctl', '-u', 'dytallix-store-standin-agent', '-o', 'cat', '--no-pager', '-n', '20'],
                         ['journalctl', '-u', 'dytallix-store-standin-fault', '-o', 'cat', '--no-pager', '-n', '20'],
                         ['tail', '-n', '40', EVENTS],
                         ['journalctl', '-k', '--no-pager', '-n', '50', '--grep', 'apparmor'],
                         ['tail', '-n', '40', '/var/log/cloud-init-output.log']):
                _, out, err = guest.run(args, check=False, timeout=60)
                say(f'$ {" ".join(args)}\n{out}{err}')
        except (Failed, OSError) as error:
            say(f'{label}: no guest agent ({error})')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    p = commands.add_parser('prepare')
    p.add_argument('--release', type=Path, required=True)
    p.add_argument('--work', type=Path, required=True)
    for name in ('network-up', 'install-vms', 'run', 'wallet-drill', 'load', 'checkpoint', 'adversary-drill',
                 'fault-drill', 'validator-recovery-drill', 'restore-drill', 'kit-drill', 'monitor-drill', 'agent-drill', 'diagnostics'):
        commands.add_parser(name).add_argument('--work', type=Path, required=True)
    r = commands.add_parser('reconcile')
    r.add_argument('--work', type=Path, required=True)
    r.add_argument('--after', choices=('wallet-drill', 'load'), default='wallet-drill')
    v = commands.add_parser('vms-up')
    v.add_argument('--work', type=Path, required=True)
    v.add_argument('--image', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'prepare':
            prepare(args.release, args.work)
            # The runner opens the backup copy with the release's own signer.
            shutil.copytree(args.release / 'bin', args.work / 'release-bin')
        elif args.command == 'network-up':
            network_up(args.work)
        elif args.command == 'vms-up':
            vms_up(args.work, args.image)
        elif args.command == 'install-vms':
            install_vms(args.work)
        elif args.command == 'run':
            run(args.work)
        elif args.command == 'wallet-drill':
            wallet_drill(args.work)
        elif args.command == 'reconcile':
            reconcile_network(args.work, args.after)
        elif args.command == 'load':
            load_drill(args.work)
        elif args.command == 'checkpoint':
            checkpoint_network(args.work)
        elif args.command == 'adversary-drill':
            adversary_drill(args.work)
        elif args.command == 'fault-drill':
            fault_drill(args.work)
        elif args.command == 'validator-recovery-drill':
            validator_recovery_drill(args.work)
        elif args.command == 'restore-drill':
            restore_drill(args.work)
        elif args.command == 'kit-drill':
            kit_drill(args.work)
        elif args.command == 'monitor-drill':
            monitor_drill(args.work)
        elif args.command == 'agent-drill':
            agent_drill(args.work)
        else:
            diagnostics(args.work)
    except (Failed, staging_host.Failed, reconcile.Failed, load_generator.Failed, checkpoints.Failed,
            faults.Failed, recovery.Failed, adversary.Failed, staging_founder.Failed, staging_command.Failed, OSError, KeyError, ValueError) as error:
        print(f'staging_network: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
