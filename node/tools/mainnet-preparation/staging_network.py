#!/usr/bin/env python3
"""Run a three-host staging network on one Ubuntu 24.04 CI runner (E05;
host setup v1, step H4): the validator on the runner itself, the sentry and
the endpoint in two KVM virtual machines on a private bridge. Each host has
its own address on its own interface, as host setup v1 requires, and each
installs from its own bundle with its install.sh. Invented records and
throwaway keys only.

  staging_network.py prepare --release DIR --work DIR
  sudo staging_network.py network-up --work DIR
  sudo staging_network.py vms-up --work DIR --image UBUNTU_24.04_CLOUD_IMAGE
  sudo staging_network.py install-vms --work DIR
  sudo staging_host.py install --work DIR/net --label validator-1 ...
  sudo staging_network.py run --work DIR
  sudo staging_network.py diagnostics --work DIR

The VMs are driven through the QEMU guest agent (a virtio serial channel):
once a node is installed, its firewall drops every other inbound packet.
The bundles reach them on their cloud-init disk. The VMs install their
packages and their nodes before the validator's install on the runner
replaces the runner's firewall rules (and with them the VMs' route out).

run starts the sentry and the endpoint and checks that both follow the
validator, that the endpoint's status page answers, that the sentry takes a
snapshot (a staging interval of 10 blocks), that its backup job uploads an
encrypted copy to a stand-in store inside the sentry VM, that the copy
opens on the runner with the backup code and holds the sentry's snapshot,
and that both VMs verify against their bundles.
"""
import argparse
import base64
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
import staging_host  # noqa: E402

BRIDGE = 'dytbr0'
ADDRESSES = {'validator-1': '10.77.0.1', 'sentry-1': '10.77.0.11', 'endpoint-1': '10.77.0.12'}
VMS = ('sentry-1', 'endpoint-1')
MACS = {'sentry-1': '52:54:00:77:00:11', 'endpoint-1': '52:54:00:77:00:12'}
TAPS = {'sentry-1': 'dyttap11', 'endpoint-1': 'dyttap12'}
SNAPSHOT_INTERVAL = 10
STANDIN_PORT = 9000
UPLOAD = {'schema': 'dytallix.backup-upload.v1', 'endpoint': f'http://127.0.0.1:{STANDIN_PORT}',
          'bucket': 'copies', 'region': 'auto', 'prefix': 'staging/', 'access_key_id': 'STANDIN',
          'secret_access_key': 'c3RhbmRpbi1vbmx5'}
METRICS = '/var/lib/dytallix/metrics/dytallix-app.prom'
SNAPSHOTS = '/var/lib/dytallix/snapshots'
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
    return staging_host.prepare(release, work / 'net', plan=network_plan(), backup_upload=upload,
                                snapshot_interval=SNAPSHOT_INTERVAL)


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
        shutil.copyfile(HERE / 's3_standin.py', seed / 's3_standin.py')
        sh('genisoimage', '-quiet', '-output', str(vm / f'{label}-seed.iso'), '-volid', 'cidata', '-joliet', '-rock',
           *[str(seed / name) for name in ('user-data', 'meta-data', 'network-config', 'bundle.tar', 's3_standin.py')])
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
        raise Failed('this runner has no /dev/kvm; the sentry and endpoint VMs need KVM')


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
                   '&& tar -xf /mnt/seed/bundle.tar -C /root/node && cp /mnt/seed/s3_standin.py /root/ '
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
    # The endpoint's status page, from the runner.
    with urllib.request.urlopen(f'http://{ADDRESSES["endpoint-1"]}:8080/status', timeout=10) as response:
        page = json.loads(response.read())
    if page.get('chain_id') != staging_host.CHAIN_ID or not int(page.get('height', 0)) > 0:
        raise Failed(f'the endpoint status page is wrong: {page}')
    say(f'== endpoint status page: {page}')
    # The sentry's snapshot and its off-host copy.
    sentry = guests['sentry-1']
    deadline = time.monotonic() + 180
    while True:
        _, listing, _ = sentry.run(['sh', '-c', f'cd {SNAPSHOTS} && ls -d [0-9]*/metadata.json 2>/dev/null'], check=False)
        heights = sorted(int(name.split('/')[0]) for name in listing.split())
        if heights:
            say(f'== sentry snapshots at {heights}')
            break
        if time.monotonic() > deadline:
            raise Failed('the sentry took no snapshot')
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
    say(f'== backup of height {copy["height"]} uploaded, opened with the backup code and matches the snapshot')
    for label, guest in guests.items():
        _, out, _ = guest.run(['python3', '-I', '-B', '/root/node/dytallix-host/host_install.py', 'verify'])
        say(f'{label}: {out.strip()}')
    say('== the staging network runs: validator, sentry and endpoint, with an off-host copy')


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
    for name in ('network-up', 'install-vms', 'run', 'diagnostics'):
        commands.add_parser(name).add_argument('--work', type=Path, required=True)
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
        else:
            diagnostics(args.work)
    except (Failed, staging_host.Failed, OSError, KeyError, ValueError) as error:
        print(f'staging_network: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
