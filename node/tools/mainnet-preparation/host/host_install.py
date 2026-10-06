#!/usr/bin/env python3
"""Install, verify or wipe a Dytallix host bundle (E05; host setup v1,
node/docs/architecture/host-setup-v1.md). Runs as root at the host console,
from the unpacked bundle, through install.sh and wipe.sh:

  install  checks the host and the bundle, creates the account and the
           directories, installs the release binaries (immutable) and every
           file as INSTALL_MANIFEST.json says, unseals the node keys with
           the seal code typed from paper, loads the AppArmor profiles in
           enforce mode and the firewall table, enables the unit, and
           verifies everything. It never touches an existing install.
  verify   re-checks an installed host against the bundle.
  wipe     removes the node from the host (staging before production):
           /opt/dytallix, /etc/dytallix, /var/lib/dytallix, the unit, the
           profiles, the firewall table and the journal setting. It keeps
           the account.

Standard library only; Ubuntu 24.04's python3.
"""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys

SCHEMA = 'dytallix.host-install.v1'
SEALED_SCHEMA = 'dytallix.sealed-host-keys.v1'
TREES = ('/opt/dytallix', '/etc/dytallix', '/var/lib/dytallix')
HOME = '/var/lib/dytallix/node'
UNIT = 'dytallix-node'
UNIT_FILE = f'/etc/systemd/system/{UNIT}.service'
PROFILE_FILE = f'/etc/apparmor.d/{UNIT}'
FIREWALL_FILE = '/etc/nftables.d/dytallix.nft'
FIREWALL_TABLE = ('inet', 'dytallix_node')
JOURNALD_FILE = '/etc/systemd/journald.conf.d/dytallix.conf'
SINGLE_FILES = (UNIT_FILE, PROFILE_FILE, FIREWALL_FILE, JOURNALD_FILE)
NFT_CONF = '/etc/nftables.conf'
NFT_INCLUDE = 'include "/etc/nftables.d/*.nft"'
UNSEALER = 'dytallix-root-sign'
TOOLS = ('apparmor_parser', 'nft', 'systemctl', 'chattr', 'lsattr', 'useradd', 'groupadd', 'getent', 'timedatectl')
NOLOGIN = '/usr/sbin/nologin'


class Refused(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Refused(message)


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


class Host:
    """The system, behind one object so that tests can run on a directory."""

    def __init__(self, root='/'):
        self.root = Path(root)

    def path(self, absolute):
        return self.root / PurePosixPath(absolute).relative_to('/')

    def run(self, args, check=True, interactive=False):
        result = subprocess.run(args, text=True, stdin=None if interactive else subprocess.DEVNULL,
                                stdout=None if interactive else subprocess.PIPE,
                                stderr=None if interactive else subprocess.PIPE)
        if check and result.returncode != 0:
            raise Refused(f'{" ".join(args)} failed: {(result.stderr or "").strip()}')
        return result

    def is_root(self):
        return os.geteuid() == 0

    def which(self, tool):
        return shutil.which(tool) is not None

    def chown(self, absolute, uid, gid):
        os.chown(self.path(absolute), uid, gid, follow_symlinks=False)

    def owner(self, absolute):
        info = os.lstat(self.path(absolute))
        return info.st_uid, info.st_gid

    def remove_tree(self, absolute):
        shutil.rmtree(self.path(absolute))


def say(message):
    print(message, flush=True)


# The bundle

def load_bundle(bundle, read=None):
    """The manifest, checked against every file the bundle carries (read
    gives a file's bytes by its path in the bundle)."""
    read = read or (lambda relative: (Path(bundle) / relative).read_bytes())
    manifest = json.loads(read('INSTALL_MANIFEST.json'))
    require(manifest.get('schema') == SCHEMA, 'not a host install manifest')
    label, release = manifest['label'], manifest['release']
    etc = f'/etc/dytallix/{release}'
    for row in manifest['files']:
        path = row['path']
        require(path.startswith(etc + '/') or path.startswith(HOME + '/') or path in SINGLE_FILES,
                f'{path} is outside the host layout')
        raw = read(f'files/{PurePosixPath(path).relative_to("/")}')
        require(len(raw) == row['bytes'] and sha256(raw) == row['sha256'], f'{path} is not the manifest\'s file')
    for binary in manifest['binaries']:
        require(binary['path'] == f'/opt/dytallix/{release}/bin/{binary["name"]}', f'{binary["name"]}: unexpected path')
        raw = read(f'bin/{binary["name"]}')
        require(sha256(raw) == binary['sha256'], f'{binary["name"]} is not the release\'s binary')
    require(any(b['name'] == UNSEALER for b in manifest['binaries']), f'the release has no {UNSEALER}')
    for directory in manifest['directories']:
        require(any(directory['path'] == tree or directory['path'].startswith(tree + '/') for tree in TREES),
                f'{directory["path"]} is outside the host layout')
    sealed = json.loads(read('keys.sealed.json'))
    require(sealed.get('schema') == SEALED_SCHEMA and sealed.get('label') == label, f'the sealed keys are not {label}\'s')
    named = {f'{HOME}/{entry["path"]}': entry['sha256'] for entry in sealed['files']}
    require(named == {s['path']: s['sha256'] for s in manifest['secrets']},
            'the sealed keys are not the files the manifest names')
    return manifest


# Checks before installing

def preflight(host):
    require(host.is_root(), 'run as root')
    release = {}
    for line in host.path('/etc/os-release').read_text().splitlines():
        if '=' in line:
            key, value = line.split('=', 1)
            release[key] = value.strip().strip('"')
    require(release.get('ID') == 'ubuntu' and release.get('VERSION_ID') == '24.04', 'this host is not Ubuntu 24.04')
    enabled = host.path('/sys/module/apparmor/parameters/enabled')
    require(enabled.exists() and enabled.read_text().strip() == 'Y', 'AppArmor is not enabled')
    missing = [tool for tool in TOOLS if not host.which(tool)]
    require(not missing, f'missing tools: {", ".join(missing)} (install apparmor, apparmor-utils and nftables)')
    require(host.path(NFT_CONF).exists(), f'{NFT_CONF} is missing (install nftables)')
    synced = host.run(['timedatectl', 'show', '-p', 'NTPSynchronized', '--value'], check=False).stdout.strip()
    require(synced == 'yes', 'the clock is not synchronized yet (systemd-timesyncd); wait and retry')
    for firewall in ('ufw', 'firewalld'):
        active = host.run(['systemctl', 'is-active', firewall], check=False).stdout.strip()
        require(active != 'active', f'{firewall} is active; disable it (the node\'s table is the host firewall)')


def existing(host):
    return [path for path in TREES + SINGLE_FILES if os.path.lexists(host.path(path))]


# The account

def account(host, manifest):
    acct = manifest['account']
    user, uid, gid = acct['user'], acct['uid'], acct['gid']
    entry = host.run(['getent', 'passwd', user], check=False)
    if entry.returncode == 0:
        fields = entry.stdout.strip().split(':')
        require(len(fields) == 7 and fields[2] == str(uid) and fields[3] == str(gid) and fields[6] == NOLOGIN,
                f'the account {user} exists with another UID, group or shell')
        return uid, gid
    require(host.run(['getent', 'passwd', str(uid)], check=False).returncode != 0, f'UID {uid} is taken')
    group = host.run(['getent', 'group', acct['group']], check=False)
    if group.returncode == 0:
        require(group.stdout.strip().split(':')[2] == str(gid), f'the group {acct["group"]} exists with another GID')
    else:
        require(host.run(['getent', 'group', str(gid)], check=False).returncode != 0, f'GID {gid} is taken')
        host.run(['groupadd', '--system', '--gid', str(gid), acct['group']])
    host.run(['useradd', '--system', '--uid', str(uid), '--gid', str(gid), '--home-dir', '/nonexistent',
              '--no-create-home', '--shell', NOLOGIN, user])
    return uid, gid


# Installing

def write_new(host, absolute, raw):
    """A new file; its parents, outside the node's trees, are made root 0755."""
    target = host.path(absolute)
    missing = [p for p in reversed(target.parents) if not p.exists()]
    for parent in missing:
        parent.mkdir()
        os.chmod(parent, 0o755)
    descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'wb') as file:
        file.write(raw)
        file.flush()
        os.fsync(file.fileno())


def ids(owner, uid, gid):
    return (uid, gid) if owner != 'root' else (0, 0)


def install(bundle, host, out=say):
    bundle = Path(bundle)
    preflight(host)
    manifest = load_bundle(bundle)
    label, release = manifest['label'], manifest['release']
    found = existing(host)
    require(not found, f'this host already has a Dytallix install ({", ".join(found)}); '
                       'wipe it first (wipe.sh, staging only)')
    out(f'Installing {label} ({manifest["role"]}), release {release}, chain {manifest["chain_id"]}')
    os.umask(0o077)
    uid, gid = account(host, manifest)
    for directory in manifest['directories']:
        host.path(directory['path']).mkdir(mode=0o700)
    for binary in manifest['binaries']:
        write_new(host, binary['path'], (bundle / 'bin' / binary['name']).read_bytes())
        os.chmod(host.path(binary['path']), 0o555)
    for row in manifest['files']:
        write_new(host, row['path'], (bundle / 'files' / PurePosixPath(row['path']).relative_to('/')).read_bytes())
    # The node keys, unsealed with the code typed from paper.
    bin_dir = f'/opt/dytallix/{release}/bin'
    out(f'Type the seal code for {label} from its paper (dytallix-seal-{label} ...), then press Enter:')
    host.run([str(host.path(f'{bin_dir}/{UNSEALER}')), 'unseal', '-paper', '-', '-sealed',
              str(bundle / 'keys.sealed.json'), '-label', label, '-out', str(host.path(HOME))], interactive=True)
    # Owners and modes, innermost first; directories last.
    for secret in manifest['secrets']:
        os.chmod(host.path(secret['path']), int(secret['mode'], 8))
        host.chown(secret['path'], *ids(secret['owner'], uid, gid))
    for row in manifest['files']:
        os.chmod(host.path(row['path']), int(row['mode'], 8))
        host.chown(row['path'], *ids(row['owner'], uid, gid))
    for binary in manifest['binaries']:
        host.chown(binary['path'], 0, 0)
    for directory in reversed(manifest['directories']):
        os.chmod(host.path(directory['path']), int(directory['mode'], 8))
        host.chown(directory['path'], *ids(directory['owner'], uid, gid))
    for binary in manifest['binaries']:
        host.run(['chattr', '+i', str(host.path(binary['path']))])
    # AppArmor, the journal, the firewall and the unit.
    host.run(['apparmor_parser', '--replace', '--write-cache', str(host.path(PROFILE_FILE))])
    if any(row['path'] == JOURNALD_FILE for row in manifest['files']):
        host.run(['systemctl', 'restart', 'systemd-journald'])
    conf = host.path(NFT_CONF)
    text = conf.read_text()
    if NFT_INCLUDE not in text.splitlines():
        conf.write_text(text + ('' if text.endswith('\n') else '\n') + NFT_INCLUDE + '\n')
    host.run(['nft', '--check', '--file', str(conf)])
    host.run(['systemctl', 'enable', 'nftables'])
    host.run(['systemctl', 'restart', 'nftables'])
    host.run(['systemctl', 'daemon-reload'])
    host.run(['systemctl', 'enable', f'{UNIT}.service'])
    problems = verify(bundle, host, manifest)
    require(not problems, 'the install does not verify:\n  ' + '\n  '.join(problems))
    out(f'Installed and verified {label}. Start the node with: systemctl start {UNIT}')
    return manifest


# Verifying

def verify(bundle, host, manifest=None):
    """Every difference between the host and the bundle, as text."""
    manifest = manifest or load_bundle(bundle)
    acct = manifest['account']
    uid, gid = acct['uid'], acct['gid']
    problems = []

    def check(absolute, mode, owner, digest=None):
        path = host.path(absolute)
        try:
            info = os.lstat(path)
        except OSError as error:
            problems.append(f'{absolute}: {error.strerror}')
            return
        if oct(info.st_mode & 0o7777) != oct(int(mode, 8)):
            problems.append(f'{absolute}: mode {oct(info.st_mode & 0o7777)}, expected {mode}')
        if host.owner(absolute) != ids(owner, uid, gid):
            problems.append(f'{absolute}: owner {host.owner(absolute)}, expected {owner}')
        if digest is not None and (not path.is_file() or path.is_symlink() or sha256(path.read_bytes()) != digest):
            problems.append(f'{absolute}: content differs')

    for directory in manifest['directories']:
        check(directory['path'], directory['mode'], directory['owner'])
    for row in manifest['files']:
        check(row['path'], row['mode'], row['owner'], row['sha256'])
    for secret in manifest['secrets']:
        check(secret['path'], secret['mode'], secret['owner'], secret['sha256'])
    for binary in manifest['binaries']:
        check(binary['path'], '0555', 'root', binary['sha256'])
        flags = host.run(['lsattr', '-d', str(host.path(binary['path']))], check=False).stdout.split()
        if not flags or 'i' not in flags[0]:
            problems.append(f'{binary["path"]}: not immutable')
    loaded = {}
    profiles = host.path('/sys/kernel/security/apparmor/profiles')
    if profiles.exists():
        for line in profiles.read_text().splitlines():
            name, _, mode = line.rpartition(' (')
            loaded[name] = mode.rstrip(')')
    for name in manifest['apparmor_profiles']:
        if loaded.get(name) != 'enforce':
            problems.append(f'AppArmor profile {name}: {loaded.get(name, "not loaded")}, expected enforce')
    if host.run(['nft', 'list', 'table', *FIREWALL_TABLE], check=False).returncode != 0:
        problems.append(f'firewall table {" ".join(FIREWALL_TABLE)} is not loaded')
    for unit in ('nftables', f'{UNIT}.service'):
        if host.run(['systemctl', 'is-enabled', unit], check=False).stdout.strip() != 'enabled':
            problems.append(f'{unit} is not enabled')
    return problems


# Wiping

def wipe(bundle, host, confirm=input, out=say):
    require(host.is_root(), 'run as root')
    manifest = json.loads((Path(bundle) / 'INSTALL_MANIFEST.json').read_bytes())
    require(manifest.get('schema') == SCHEMA, 'not a host install manifest')
    label = manifest['label']
    out(f'This removes the Dytallix node from this host: {", ".join(TREES)} (its node keys and chain data), '
        f'the unit, the AppArmor profiles, the firewall table and the journal setting.')
    answer = confirm(f'Type "wipe {label}" to continue: ').strip()
    require(answer == f'wipe {label}', 'not confirmed; nothing removed')
    host.run(['systemctl', 'disable', '--now', f'{UNIT}.service'], check=False)
    opt = host.path('/opt/dytallix')
    if opt.exists():
        for binary in sorted(opt.glob('*/bin/*')):
            host.run(['chattr', '-i', str(binary)])
    if os.path.lexists(host.path(PROFILE_FILE)):
        host.run(['apparmor_parser', '--remove', str(host.path(PROFILE_FILE))], check=False)
    if host.run(['nft', 'list', 'table', *FIREWALL_TABLE], check=False).returncode == 0:
        host.run(['nft', 'delete', 'table', *FIREWALL_TABLE])
    for path in SINGLE_FILES:
        if os.path.lexists(host.path(path)):
            os.unlink(host.path(path))
    host.run(['systemctl', 'daemon-reload'])
    host.run(['systemctl', 'restart', 'systemd-journald'], check=False)
    for tree in TREES:
        if os.path.lexists(host.path(tree)):
            host.remove_tree(tree)
    remaining = existing(host)
    require(not remaining, f'still present: {", ".join(remaining)}')
    out(f'Wiped {label}. The account {manifest["account"]["user"]} is kept.')


def main(argv):
    if len(argv) != 2 or argv[1] not in ('install', 'verify', 'wipe'):
        print('usage: host_install.py install|verify|wipe', file=sys.stderr)
        return 2
    bundle = Path(__file__).resolve().parent
    host = Host()
    try:
        if argv[1] == 'install':
            install(bundle, host)
        elif argv[1] == 'verify':
            require(host.is_root(), 'run as root')
            problems = verify(bundle, host)
            print(json.dumps({'status': 'FAIL' if problems else 'PASS', 'problems': problems}, indent=2))
            return 1 if problems else 0
        else:
            wipe(bundle, host)
    except (Refused, OSError, KeyError, ValueError) as error:
        print(f'host_install: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv))
