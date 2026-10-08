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
  verify   re-checks an installed host against the bundle. The validator's
           signing state changes as it signs, so after the install only its
           owner and mode are checked.
  stage    installs a new release beside the running one from this host's
           bundle for that release: its binaries, configuration and the
           unit, profiles, firewall and journal setting it switches to. The
           node home and keys are kept and nothing is unsealed. Releases
           older than the active one are removed.
  switch   once the old release has stopped (at the activation height),
           installs the staged unit, profiles, firewall and journal setting,
           verifies the host against the new bundle and starts the node.
  restore  on a freshly installed host, before its first start, restores
           the node from an off-host copy (disaster recovery v1, R5): opens
           it with the backup code typed from paper, starts the node once
           with --restore-snapshot, waits until it has restored and caught
           up, then removes the one-time start and the opened copy. On the
           validator it requires --accept-history-loss.
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
import re
import shutil
import stat
import subprocess
import sys
import time

SCHEMA = 'dytallix.host-install.v1'
SEALED_SCHEMA = 'dytallix.sealed-host-keys.v1'
# The sentry's backup secrets and state (disaster recovery v1) live outside
# the node's trees.
BACKUP_ETC = '/etc/dytallix-backup'
TREES = ('/opt/dytallix', '/etc/dytallix', '/var/lib/dytallix', BACKUP_ETC, '/var/lib/dytallix-backup')
HOME = '/var/lib/dytallix/node'
UNIT = 'dytallix-node'
UNIT_FILE = f'/etc/systemd/system/{UNIT}.service'
PROFILE_FILE = f'/etc/apparmor.d/{UNIT}'
FIREWALL_FILE = '/etc/nftables.d/dytallix.nft'
FIREWALL_TABLE = ('inet', 'dytallix_node')
JOURNALD_FILE = '/etc/systemd/journald.conf.d/dytallix.conf'
BACKUP_UNIT = 'dytallix-backup'
SINGLE_FILES = (UNIT_FILE, PROFILE_FILE, FIREWALL_FILE, JOURNALD_FILE,
                f'/etc/systemd/system/{BACKUP_UNIT}.service', f'/etc/systemd/system/{BACKUP_UNIT}.timer')
# The sealed keys are unsealed here first, root-only; each file then goes
# where the manifest says, and this directory is removed.
UNSEALED = '/var/lib/dytallix/.unsealed'
NFT_CONF = '/etc/nftables.conf'
UFW_CONF = '/etc/ufw/ufw.conf'
NFT_INCLUDE = 'include "/etc/nftables.d/*.nft"'
UNSEALER = 'dytallix-root-sign'
# The signing state changes each time a validator signs; only its install is
# checked against the bundle.
SIGNING_STATE = f'{HOME}/data/priv_validator_state.json'
# Each release keeps the manifest it was installed from, and a staged
# release the unit, profiles, firewall and journal setting it switches to.
INSTALLED = 'install-manifest.json'
NEXT = 'next'
# A one-time restore (disaster recovery v1, R5): the opened copy, readable
# by the node, and the start that passes it, under /run so that a reboot
# also removes it.
RESTORE = '/var/lib/dytallix/restore'
RESTORE_COPY = f'{RESTORE}/copy'
RESTORE_DROP_IN = f'/run/systemd/system/{UNIT}.service.d/50-restore.conf'
# Above the approved catch-up budget (6 h), which the supervisor waits.
RESTORE_TIMEOUT = 7 * 3600
TOOLS = ('apparmor_parser', 'nft', 'systemctl', 'chattr', 'lsattr', 'useradd', 'groupadd', 'getent', 'timedatectl')
NOLOGIN = '/usr/sbin/nologin'


class Refused(Exception):
    pass


def secret_path(relative):
    """Where a sealed file is installed: node keys in the node home, the
    sentry's backup secrets outside it."""
    if relative.startswith('backup/'):
        return f'{BACKUP_ETC}/{relative[len("backup/"):]}'
    return f'{HOME}/{relative}'


def sealed_name(absolute):
    """A secret file's name in the sealed keys."""
    if absolute.startswith(BACKUP_ETC + '/'):
        return 'backup/' + absolute[len(BACKUP_ETC) + 1:]
    require(absolute.startswith(HOME + '/'), f'{absolute} is not a sealed file')
    return absolute[len(HOME) + 1:]


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

    def mode(self, absolute):
        return stat.S_IMODE(os.lstat(self.path(absolute)).st_mode)

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
    named = {secret_path(entry['path']): entry['sha256'] for entry in sealed['files']}
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
    # The node runs the root helper only from a path whose every ancestor is
    # root-owned without group or other write (static_helper.rs); the
    # installer makes /opt/dytallix and below, the host provides / and /opt.
    for ancestor in ('/', '/opt'):
        mode = host.mode(ancestor)
        require(host.owner(ancestor)[0] == 0 and mode & 0o022 == 0,
                f'{ancestor} must be root-owned without group or other write (it is uid '
                f'{host.owner(ancestor)[0]}, mode {mode:04o}); the node refuses its root helper otherwise')
    missing = [tool for tool in TOOLS if not host.which(tool)]
    require(not missing, f'missing tools: {", ".join(missing)} (install apparmor, apparmor-utils and nftables)')
    require(host.path(NFT_CONF).exists(), f'{NFT_CONF} is missing (install nftables)')
    synced = host.run(['timedatectl', 'show', '-p', 'NTPSynchronized', '--value'], check=False).stdout.strip()
    require(synced == 'yes', 'the clock is not synchronized yet (systemd-timesyncd); wait and retry')
    # ufw's unit is a oneshot that stays active after `ufw disable`; ufw.conf
    # says whether it filters, now and at boot. firewalld is a daemon.
    ufw = host.path(UFW_CONF)
    require(not (ufw.exists() and re.search(r'^ENABLED=yes\s*$', ufw.read_text(), re.M)),
            'ufw is enabled; run `ufw disable` (the node\'s table is the host firewall)')
    active = host.run(['systemctl', 'is-active', 'firewalld'], check=False).stdout.strip()
    require(active != 'active', 'firewalld is active; disable it (the node\'s table is the host firewall)')


def existing(host):
    return [path for path in TREES + SINGLE_FILES if os.path.lexists(host.path(path))]


def etc(release):
    return f'/etc/dytallix/{release}'


def active_release(host):
    """The release the unit runs, from its ExecStart."""
    found = re.search(r'^ExecStart=/opt/dytallix/([0-9a-f]{16})/bin/', host.path(UNIT_FILE).read_text(), re.M)
    require(found, 'the unit names no installed release')
    return found.group(1)


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
    write_new(host, f'{etc(release)}/{INSTALLED}', (bundle / 'INSTALL_MANIFEST.json').read_bytes())
    # The node keys (and the sentry's backup secrets), unsealed with the code
    # typed from paper into a root-only directory, then placed.
    bin_dir = f'/opt/dytallix/{release}/bin'
    staging = host.path(UNSEALED)
    staging.mkdir(mode=0o700)
    for name in ('config', 'data', 'backup'):
        (staging / name).mkdir(mode=0o700)
    out(f'Type the seal code for {label} from its paper (dytallix-seal-{label} ...), then press Enter:')
    host.run([str(host.path(f'{bin_dir}/{UNSEALER}')), 'unseal', '-paper', '-', '-sealed',
              str(bundle / 'keys.sealed.json'), '-label', label, '-out', str(staging)], interactive=True)
    for secret in manifest['secrets']:
        write_new(host, secret['path'], (staging / sealed_name(secret['path'])).read_bytes())
    host.remove_tree(UNSEALED)
    # Owners and modes, innermost first; directories last.
    for secret in manifest['secrets']:
        os.chmod(host.path(secret['path']), int(secret['mode'], 8))
        host.chown(secret['path'], *ids(secret['owner'], uid, gid))
    for row in manifest['files']:
        os.chmod(host.path(row['path']), int(row['mode'], 8))
        host.chown(row['path'], *ids(row['owner'], uid, gid))
    os.chmod(host.path(f'{etc(release)}/{INSTALLED}'), 0o444)
    host.chown(f'{etc(release)}/{INSTALLED}', 0, 0)
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
    for timer in manifest.get('timers', []):
        host.run(['systemctl', 'enable', '--now', timer])
    problems = verify(bundle, host, manifest, fresh=True)
    require(not problems, 'the install does not verify:\n  ' + '\n  '.join(problems))
    out(f'Installed and verified {label}. Start the node with: systemctl start {UNIT}')
    return manifest


# Moving to a new release (P01, 6 October 2026: stage alongside, switch at
# the halt)

def stage(bundle, host, out=say):
    bundle = Path(bundle)
    preflight(host)
    manifest = load_bundle(bundle)
    release, current = manifest['release'], active_release(host)
    installed = json.loads(host.path(f'{etc(current)}/{INSTALLED}').read_bytes())
    for key in ('label', 'role', 'chain_id', 'account', 'unit'):
        require(manifest[key] == installed[key], f'the bundle is for another host or chain ({key} differs)')
    require(release != current, f'release {release} is already active')

    def home(m):
        return {row['path']: row['sha256'] for row in m['files'] if row['path'].startswith(HOME + '/')}
    require(home(manifest) == home(installed), 'a release switch never changes the node home files')
    require({s['path']: s['sha256'] for s in manifest['secrets']} == {s['path']: s['sha256'] for s in installed['secrets']},
            'a release switch keeps the node keys')
    # Releases other than the active one go, including an earlier attempt
    # to stage this one.
    for tree in ('/opt/dytallix', '/etc/dytallix'):
        for entry in sorted(host.path(tree).iterdir()):
            if entry.name != current:
                for binary in sorted(entry.glob('bin/*')):
                    host.run(['chattr', '-i', str(binary)])
                host.remove_tree(f'{tree}/{entry.name}')
                out(f'removed {tree}/{entry.name}')
    out(f'Staging release {release} beside {current} for {manifest["label"]}')
    os.umask(0o077)
    acct = manifest['account']
    uid, gid = acct['uid'], acct['gid']
    roots = (f'/opt/dytallix/{release}', etc(release))
    directories = [d for d in manifest['directories'] if d['path'] in roots or d['path'].startswith(roots[0] + '/')
                   or d['path'].startswith(roots[1] + '/')]
    for directory in directories:
        host.path(directory['path']).mkdir(mode=0o700)
    for binary in manifest['binaries']:
        write_new(host, binary['path'], (bundle / 'bin' / binary['name']).read_bytes())
        os.chmod(host.path(binary['path']), 0o555)
    staged = []
    for row in manifest['files']:
        raw = (bundle / 'files' / PurePosixPath(row['path']).relative_to('/')).read_bytes()
        if row['path'].startswith(roots[1] + '/'):
            target = row['path']
        elif row['path'] in SINGLE_FILES:
            target = f'{etc(release)}/{NEXT}/{PurePosixPath(row["path"]).name}'
        else:
            continue
        write_new(host, target, raw)
        staged.append((target, row))
    write_new(host, f'{etc(release)}/{INSTALLED}', (bundle / 'INSTALL_MANIFEST.json').read_bytes())
    for target, row in staged:
        os.chmod(host.path(target), int(row['mode'], 8))
        host.chown(target, *ids(row['owner'], uid, gid))
    os.chmod(host.path(f'{etc(release)}/{INSTALLED}'), 0o444)
    host.chown(f'{etc(release)}/{INSTALLED}', 0, 0)
    os.chmod(host.path(f'{etc(release)}/{NEXT}'), 0o755)
    host.chown(f'{etc(release)}/{NEXT}', 0, 0)
    for binary in manifest['binaries']:
        host.chown(binary['path'], 0, 0)
    for directory in reversed(directories):
        os.chmod(host.path(directory['path']), int(directory['mode'], 8))
        host.chown(directory['path'], *ids(directory['owner'], uid, gid))
    for binary in manifest['binaries']:
        host.run(['chattr', '+i', str(host.path(binary['path']))])
    for target, row in staged:
        require(sha256(host.path(target).read_bytes()) == row['sha256'], f'{target} is not the bundle\'s file')
    out(f'Staged {release}. When {current} stops at the activation height, run switch.')
    return manifest


def replace_file(host, absolute, raw, mode):
    """Replaces a root-owned file in one step."""
    target = host.path(absolute)
    temporary = target.with_name(target.name + '.dytallix-new')
    if os.path.lexists(temporary):
        os.unlink(temporary)
    write_new(host, str(PurePosixPath(absolute).with_name(target.name + '.dytallix-new')), raw)
    os.chmod(temporary, mode)
    host.chown(str(PurePosixPath(absolute).with_name(target.name + '.dytallix-new')), 0, 0)
    os.replace(temporary, target)
    host.chown(absolute, 0, 0)


def switch(bundle, host, out=say):
    bundle = Path(bundle)
    preflight(host)
    manifest = load_bundle(bundle)
    release, current = manifest['release'], active_release(host)
    require(release != current, f'release {release} is already active')
    require(host.path(f'{etc(release)}/{INSTALLED}').exists(), f'release {release} is not staged; run stage first')
    state = host.run(['systemctl', 'is-active', f'{UNIT}.service'], check=False).stdout.strip()
    require(state not in ('active', 'activating', 'reloading', 'deactivating'),
            f'{UNIT} is {state}; switch only once release {current} has stopped')
    out(f'Switching {manifest["label"]} from {current} to {release}')
    rows = {row['path']: row for row in manifest['files']}
    # The old release's profiles have release-specific names; unload them
    # while nothing runs under them, before the new file replaces theirs.
    host.run(['apparmor_parser', '--remove', str(host.path(PROFILE_FILE))])
    for path in SINGLE_FILES:
        if path not in rows:
            continue
        raw = host.path(f'{etc(release)}/{NEXT}/{PurePosixPath(path).name}').read_bytes()
        require(sha256(raw) == rows[path]['sha256'], f'the staged {path} is not the bundle\'s')
        replace_file(host, path, raw, int(rows[path]['mode'], 8))
    host.run(['apparmor_parser', '--replace', '--write-cache', str(host.path(PROFILE_FILE))])
    if JOURNALD_FILE in rows:
        host.run(['systemctl', 'restart', 'systemd-journald'])
    host.run(['nft', '--check', '--file', str(host.path(NFT_CONF))])
    host.run(['systemctl', 'restart', 'nftables'])
    host.run(['systemctl', 'daemon-reload'])
    host.run(['systemctl', 'enable', f'{UNIT}.service'])
    for timer in manifest.get('timers', []):
        host.run(['systemctl', 'enable', '--now', timer])
    problems = verify(bundle, host, manifest)
    require(not problems, 'the switched host does not verify; the node was not started:\n  ' + '\n  '.join(problems))
    host.run(['systemctl', 'start', f'{UNIT}.service'])
    out(f'Switched to {release} and started {UNIT}. Release {current} stays installed until the next stage.')
    return manifest


# Verifying

def verify(bundle, host, manifest=None, fresh=False):
    """Every difference between the host and the bundle, as text. Unless
    fresh (just installed), the signing state's contents are not compared."""
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
        check(secret['path'], secret['mode'], secret['owner'],
              secret['sha256'] if fresh or secret['path'] != SIGNING_STATE else None)
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
    for unit in ('nftables', f'{UNIT}.service', *manifest.get('timers', [])):
        if host.run(['systemctl', 'is-enabled', unit], check=False).stdout.strip() != 'enabled':
            problems.append(f'{unit} is not enabled')
    return problems


# Wiping

# Restoring from an off-host copy (disaster recovery v1, R5)

def restored_report(journal, height):
    """The supervisor's report once the node has started at or above the
    copy's height, if the journal holds it."""
    for line in journal.splitlines():
        if not line.startswith('{'):
            continue
        try:
            report = json.loads(line)
        except ValueError:
            continue
        ready = report.get('engine_readiness') if isinstance(report, dict) else None
        if report.get('started') is True and isinstance(ready, dict) and ready.get('application_height', 0) >= height:
            return report
    return None


def synced(raw):
    """(height, catching up) from the engine's operator status."""
    try:
        info = json.loads(raw)['result']['sync_info']
        return int(info['latest_block_height']), bool(info['catching_up'])
    except (ValueError, KeyError, TypeError):
        return 0, True


def restore(bundle, host, copy, accept_history_loss=False, out=say, sleep=time.sleep, clock=time.time,
            timeout=RESTORE_TIMEOUT):
    require(host.is_root(), 'run as root')
    manifest = load_bundle(Path(bundle))
    label, release, chain = manifest['label'], manifest['release'], manifest['chain_id']
    require(active_release(host) == release, f'release {release} is not installed here; install this host first')
    if manifest['role'] == 'validator':
        require(accept_history_loss, 'restoring the validator loses every block after the copy; add '
                                     '--accept-history-loss only after the incident decision (disaster recovery runbook)')
    state = host.run(['systemctl', 'is-active', f'{UNIT}.service'], check=False).stdout.strip()
    require(state != 'active', f'stop the node first (systemctl stop {UNIT})')
    # A fresh install has the application's empty database directory and no
    # engine stores.
    appdb = host.path(f'{HOME}/appdb')
    held = [path for path in (f'{HOME}/data/blockstore.db', f'{HOME}/data/state.db') if os.path.lexists(host.path(path))]
    held += [f'{HOME}/appdb'] if appdb.is_dir() and any(appdb.iterdir()) else []
    require(not held, f'{", ".join(held)} holds chain state: a restore needs a freshly installed host '
                      '(wipe and install it first)')
    require(not os.path.lexists(host.path(RESTORE_COPY)), f'{RESTORE_COPY} is left from an earlier restore; remove it')
    copy = Path(copy).resolve()
    require(copy.is_file(), f'{copy} is not a copy file')
    signer = str(host.path(f'/opt/dytallix/{release}/bin/{UNSEALER}'))
    out(f'Type the backup code for {chain} from its paper (dytallix-backup-{chain} ...), then press Enter:')
    host.run([signer, 'backup-open', '-paper', '-', '-in', str(copy), '-out', str(host.path(RESTORE_COPY))],
             interactive=True)
    # The snapshot is the chain's public state: the node reads it, only root
    # writes it.
    for dirpath, _, filenames in os.walk(host.path(RESTORE_COPY)):
        os.chmod(dirpath, 0o755)
        for name in filenames:
            os.chmod(os.path.join(dirpath, name), 0o644)
    metadata = json.loads(host.path(f'{RESTORE_COPY}/metadata.json').read_bytes())
    require(metadata.get('chain_id') == chain, f'the copy is not of {chain}')
    height = int(metadata['height'])
    out(f'Opened the copy of {chain} at height {height}. Starting {label} once to restore it.')
    exec_start = re.findall(r'^ExecStart=(.+)$', host.path(UNIT_FILE).read_text(), re.M)
    require(len(exec_start) == 1, 'the unit has no single ExecStart')
    drop_in = host.path(RESTORE_DROP_IN)
    drop_in.parent.mkdir(parents=True, exist_ok=True)
    drop_in.write_text(f'[Service]\nExecStart=\nExecStart={exec_start[0]} --restore-snapshot {RESTORE_COPY}\n')
    host.run(['systemctl', 'daemon-reload'])
    since = int(clock())
    report = None
    try:
        host.run(['systemctl', 'start', f'{UNIT}.service'])
        deadline = since + timeout
        while report is None:
            journal = host.run(['journalctl', '-u', f'{UNIT}.service', '--since', f'@{since}', '-o', 'cat',
                                '--no-pager'], check=False).stdout
            report = restored_report(journal, height)
            if report is not None:
                break
            state = host.run(['systemctl', 'is-active', f'{UNIT}.service'], check=False).stdout.strip()
            require(state in ('active', 'activating'), f'the restore start stopped ({state}); see journalctl -u '
                                                      f'{UNIT}, then wipe and install the host before trying again')
            if clock() > deadline:
                host.run(['systemctl', 'stop', f'{UNIT}.service'], check=False)
                raise Refused(f'the node did not restore and catch up within {timeout} s; it is stopped')
            sleep(15)
    finally:
        os.unlink(drop_in)
        host.run(['systemctl', 'daemon-reload'])
    host.remove_tree(RESTORE_COPY)
    # The supervisor reports ready once the engine answers, which after a
    # restore can be before block sync has moved: wait until the engine has
    # synced past the copy and is not catching up.
    rpc = str(host.path(f'/opt/dytallix/{release}/bin/dytallix-operator-rpc'))
    while True:
        status = host.run([rpc, '--home', HOME, 'status'], check=False)
        reached, catching_up = synced(status.stdout) if status.returncode == 0 else (0, True)
        if reached > height and not catching_up:
            break
        require(clock() <= deadline, f'the node restored at height {height} but has not caught up '
                                     f'(height {reached}); see journalctl -u {UNIT}')
        sleep(5)
    out(f'Restored {label} from the copy at height {height}; it has caught up to height {reached} and runs.')
    return report


def wipe(bundle, host, confirm=input, out=say):
    require(host.is_root(), 'run as root')
    manifest = json.loads((Path(bundle) / 'INSTALL_MANIFEST.json').read_bytes())
    require(manifest.get('schema') == SCHEMA, 'not a host install manifest')
    label = manifest['label']
    out(f'This removes the Dytallix node from this host: {", ".join(TREES)} (its node keys and chain data), '
        f'the unit, the AppArmor profiles, the firewall table, the journal setting and any backup unit.')
    answer = confirm(f'Type "wipe {label}" to continue: ').strip()
    require(answer == f'wipe {label}', 'not confirmed; nothing removed')
    host.run(['systemctl', 'disable', '--now', f'{UNIT}.service'], check=False)
    for unit in (f'{BACKUP_UNIT}.timer', f'{BACKUP_UNIT}.service'):
        if os.path.lexists(host.path(f'/etc/systemd/system/{unit}')):
            host.run(['systemctl', 'disable', '--now', unit], check=False)
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
    restoring = len(argv) in (3, 4) and argv[1] == 'restore' and argv[3:] in ([], ['--accept-history-loss'])
    if not restoring and (len(argv) != 2 or argv[1] not in ('install', 'verify', 'stage', 'switch', 'wipe')):
        print('usage: host_install.py install|verify|stage|switch|wipe, or restore COPY [--accept-history-loss]',
              file=sys.stderr)
        return 2
    bundle = Path(__file__).resolve().parent
    host = Host()
    try:
        if restoring:
            restore(bundle, host, argv[2], accept_history_loss=len(argv) == 4)
        elif argv[1] == 'install':
            install(bundle, host)
        elif argv[1] == 'stage':
            stage(bundle, host)
        elif argv[1] == 'switch':
            switch(bundle, host)
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
