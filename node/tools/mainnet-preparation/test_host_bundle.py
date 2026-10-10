"""Host setup H3 on a synthetic staging network: the offline key step with
stand-in key tools, the host files, the bundle, and the installer run
against a temporary root with a stand-in system. The real key tools and
`dytallix-root-sign seal`/`unseal` have their own tests; a real Ubuntu
install is H4. No real key, host or approval."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys
import tarfile
from types import SimpleNamespace
import unittest
from unittest import mock

import host_bundle
import host_files
import host_keys
import test_host_files
from test_host_files import release_manifest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'host'))
import host_install as hi  # noqa: E402

# Stand-ins for the release's key tools: the same files and outputs, with
# synthetic keys.
FAKE_TOOL = r'''#!PYTHON
import base64, hashlib, json, os, sys
name, args = os.path.basename(sys.argv[0]), sys.argv[1:]
flags = dict(zip(args[1::2], args[2::2]))
def create(path, raw):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(fd, raw); os.close(fd)
def key(seed):
    return base64.b64encode(hashlib.shake_256(seed).digest(1952)).decode()
if name == 'dytallix-peer-seed':
    seed = os.urandom(32); create(os.path.join(flags['--home'], 'config', 'pqc_peer_seed.bin'), seed)
    print(json.dumps({'version': 1, 'public_key_base64': key(seed)}))
elif name == 'dytallix-validator-key':
    seed = os.urandom(32)
    create(flags['--state-file'], b'{"height":"0","round":0,"step":0}')
    create(flags['--key-file'], json.dumps({'synthetic': seed.hex()}).encode())
    print(json.dumps({'version': 1, 'public_key_base64': key(seed)}))
elif name == 'dytallix-channel-key' and args[0] == 'generate':
    create(flags['--seed-file'], os.urandom(32)); print('{}')
elif name == 'dytallix-channel-key':
    create(flags['--output'], json.dumps({'network': flags['--network'], 'address': flags['--address']}).encode())
    print('{}')
elif name == 'dytallix-root-sign' and args[0] == 'seal':
    label, home, out = args[2], args[4], args[6]
    files = [{'path': p, 'bytes': os.path.getsize(os.path.join(home, p)),
              'sha256': hashlib.sha256(open(os.path.join(home, p), 'rb').read()).hexdigest()} for p in sorted(args[7:])]
    create(out, json.dumps({'schema': 'dytallix.sealed-host-keys.v1', 'label': label, 'files': files,
                            'nonce_hex': '00' * 12, 'ciphertext_hex': 'synthetic'}).encode())
    print('seal code for %s (write it on paper twice, then run seal-check):' % label)
    print('dytallix-seal-%s %s' % (label, ' '.join(['0000'] * 17)))
elif name == 'dytallix-root-sign' and args[0] == 'seal-check':
    pass
elif name == 'dytallix-root-sign' and args[0] == 'backup-code':
    line = 'dytallix-backup-%s %s' % (flags['-chain'], ' '.join(['00b1'] * 17))
    create(flags['-out'], (line + '\n').encode())
    print('backup code for %s (write it on paper twice):' % flags['-chain'])
    print(line)
else:
    sys.exit('unexpected ' + name)
'''


def fake_tools(directory):
    directory.mkdir()
    for name in ('dytallix-peer-seed', 'dytallix-validator-key', 'dytallix-channel-key', 'dytallix-root-sign'):
        path = directory / name
        path.write_text(FAKE_TOOL.replace('PYTHON', sys.executable))
        path.chmod(0o755)
    return directory


class FakeHost(hi.Host):
    """Ubuntu 24.04 with AppArmor and nftables, in a directory: commands
    change recorded state, and unseal copies the staging home's files."""

    def __init__(self, root, staging, profiles):
        super().__init__(root)
        self.staging, self.profiles = Path(staging), profiles
        self.commands, self.owners, self.immutable, self.enabled, self.active = [], {}, set(), set(), set()
        self.user = self.group = self.table = False
        self.loaded = set()
        self.journal, self.copy_metadata, self.synced_height = '', {}, 31
        for path, text in {'/etc/os-release': 'ID=ubuntu\nVERSION_ID="24.04"\n',
                           '/sys/module/apparmor/parameters/enabled': 'Y\n',
                           '/etc/nftables.conf': '#!/usr/sbin/nft -f\nflush ruleset\n'}.items():
            self.path(path).parent.mkdir(parents=True, exist_ok=True)
            self.path(path).write_text(text)
        for base in ('/opt', '/var/lib', '/etc/systemd/system', '/etc/apparmor.d'):
            self.path(base).mkdir(parents=True, exist_ok=True)
        for base in ('/', '/opt'):
            os.chmod(self.path(base), 0o755)
            self.owners[base] = (0, 0)

    def is_root(self):
        return True

    def which(self, tool):
        return True

    def chown(self, absolute, uid, gid):
        self.owners[absolute] = (uid, gid)

    def owner(self, absolute):
        return self.owners.get(absolute, (-1, -1))

    def remove_tree(self, absolute):
        for dirpath, _, _ in os.walk(self.path(absolute)):
            os.chmod(dirpath, 0o755)
        super().remove_tree(absolute)

    def run(self, args, check=True, interactive=False):
        self.commands.append(args)
        name, out, code = Path(args[0]).name, '', 0
        if name == 'getent':
            exists = self.user if args[1] == 'passwd' else self.group
            if exists and args[2] in ('dytallix', '41001'):
                out = ('dytallix:x:41001:41001::/nonexistent:/usr/sbin/nologin' if args[1] == 'passwd'
                       else 'dytallix:x:41001:')
            else:
                code = 2
        elif name == 'groupadd':
            self.group = True
        elif name == 'useradd':
            self.user = True
        elif name == 'timedatectl':
            out = 'yes'
        elif name == 'systemctl':
            verb = args[1]
            if verb == 'is-active':
                out = 'active' if args[2] in self.active else 'inactive'
            elif verb == 'start':
                self.active.add(args[2])
            elif verb == 'stop':
                self.active.discard(args[2])
            elif verb == 'enable':
                self.enabled.add(args[-1])
                if '--now' in args:
                    self.active.add(args[-1])
            elif verb == 'disable':
                self.enabled.discard(args[-1])
                self.active.discard(args[-1])
            elif verb == 'is-enabled':
                out = 'enabled' if args[2] in self.enabled else 'disabled'
            elif verb == 'restart' and args[2] == 'nftables':
                self.table = NFT_INCLUDE_PRESENT(self) and os.path.exists(self.path(hi.FIREWALL_FILE))
        elif name == 'chattr':
            (self.immutable.add if args[1] == '+i' else self.immutable.discard)(args[2])
        elif name == 'lsattr':
            out = ('----i---------e------- ' if args[2] in self.immutable else '--------------e------- ') + args[2]
        elif name == 'apparmor_parser':
            # Loads or unloads the profiles the file declares.
            names = re.findall(r'^profile (\S+) \{', Path(args[-1]).read_text(), re.M)
            self.loaded = (self.loaded | set(names)) if args[1] == '--replace' else (self.loaded - set(names))
            listing = self.path('/sys/kernel/security/apparmor/profiles')
            listing.parent.mkdir(parents=True, exist_ok=True)
            listing.write_text(''.join(f'{p} (enforce)\n' for p in sorted(self.loaded)))
        elif name == 'nft':
            if args[1] == 'list':
                code = 0 if self.table else 1
            elif args[1] == 'delete':
                self.table = False
        elif name == 'journalctl':
            out = self.journal
        elif name == 'dytallix-operator-rpc':
            out = json.dumps({'result': {'sync_info': {'latest_block_height': str(self.synced_height),
                                                       'catching_up': self.synced_height < 30}}})
        elif name == 'dytallix-root-sign' and args[1] == 'backup-open':
            # Writes the opened copy owner-only, as backup-open does.
            target = Path(args[args.index('-out') + 1])
            target.mkdir(mode=0o700)
            (target / 'light-blocks').mkdir(mode=0o700)
            for path, raw in (('metadata.json', json.dumps(self.copy_metadata)), ('chunk-000000', 'chunk'),
                              ('light-blocks/00000000000000000020.block', 'block')):
                (target / path).write_text(raw)
                os.chmod(target / path, 0o600)
        elif name == 'dytallix-root-sign' and args[1] == 'unseal':
            label, home = args[args.index('-label') + 1], Path(args[args.index('-out') + 1])
            source = self.staging / label
            for path in sorted(p for p in source.rglob('*') if p.is_file()):
                target = home / path.relative_to(source)
                fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                os.write(fd, path.read_bytes())
                os.close(fd)
        if check and code:
            raise hi.Refused(f'{" ".join(args)} failed')
        return SimpleNamespace(returncode=code, stdout=out, stderr='')


def NFT_INCLUDE_PRESENT(host):
    return hi.NFT_INCLUDE in host.path(hi.NFT_CONF).read_text().splitlines()


UPLOAD = {'schema': 'dytallix.backup-upload.v1', 'endpoint': 'https://storage.example', 'bucket': 'dytallix-copies',
          'region': 'auto', 'prefix': 'staging/', 'access_key_id': 'AKIDEXAMPLE', 'secret_access_key': 'c2VjcmV0'}


class HostNetwork(unittest.TestCase):
    """The synthetic network, its keys, host files and helpers."""
    # With an upload key the sentry also seals the backup secrets.
    backup = False

    def setUp(self):
        # The host file generator's synthetic network, with the root signer
        # in the release.
        with mock.patch.object(test_host_files, 'BINARIES', test_host_files.BINARIES + ['dytallix-root-sign']):
            self.network = test_host_files.HostFilesTests('test_written_out')
            self.network.setUp()
        self.addCleanup(self.network.doCleanups)
        self.tmp = Path(self.network.tmp.name)
        record = json.loads((self.network.dirs['release'] / 'BUILD_RECORD.json').read_bytes())
        # Release binaries whose digests are the record's (sha256 of the name).
        for name in record['members']:
            (self.network.dirs['release'] / 'bin').mkdir(exist_ok=True)
            (self.network.dirs['release'] / 'bin' / name).write_bytes(name.encode())
        # The offline key step, with stand-in tools.
        self.tools = fake_tools(self.tmp / 'tools')
        plan_path = self.tmp / 'plan.json'
        plan_path.write_text(json.dumps(self.network.plan))
        self.keys = self.tmp / 'public'
        self.staging = self.tmp / 'staging'
        shown = []
        upload = None
        if self.backup:
            upload = self.tmp / 'upload.json'
            descriptor = os.open(upload, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'w') as file:
                json.dump(UPLOAD, file)
        typed = lambda: next(line for line in reversed(shown) if line.startswith('dytallix-backup-'))  # noqa: E731
        self.summaries = host_keys.run(plan_path, self.tools, self.staging, self.keys, show=lambda text: shown.extend(
            str(text).splitlines()), backup_upload=upload, read_line=typed)
        self.shown = '\n'.join(shown)
        self.plan = json.loads((self.keys / 'PIN_PLAN.json').read_bytes())
        generated = host_files.generate(self.network.dirs['release'], self.network.dirs['chain'],
                                        self.network.dirs['hosts'], self.plan, self.keys,
                                        self.network.values, self.network.setup)
        self.host_files = self.tmp / 'host-files'
        host_files.write(self.host_files, generated)
        self.manifests = {label: manifest for label, (_, manifest) in generated.items()}

    def bundle(self, label, name=None):
        out = self.tmp / (name or f'{label}.bundle.tar')
        return out, host_bundle.build(self.host_files / label, self.network.dirs['release'],
                                      self.keys / f'{label}.sealed.json', out)

    def unpack(self, out):
        target = self.tmp / ('unpacked-' + out.stem)
        with tarfile.open(out) as tar:
            tar.extractall(target, filter='data')
        return target / host_bundle.PREFIX

    def install(self, label):
        out, result = self.bundle(label)
        bundle = self.unpack(out)
        manifest = self.manifests[label]
        host = FakeHost(self.tmp / f'root-{label}', self.staging, manifest['apparmor_profiles'])
        printed = []
        hi.install(bundle, host, out=printed.append)
        return bundle, host, manifest, printed


class HostBundleTests(HostNetwork):
    def test_key_step_writes_public_records_and_a_filled_plan(self):
        self.assertEqual(sorted(self.summaries), sorted(test_host_files.HOSTS))
        for label, summary in self.summaries.items():
            role = test_host_files.HOSTS[label][0]
            expected = set(host_keys.SECRETS) | ({host_keys.CHANNEL_SEED} if role == 'endpoint' else set())
            self.assertEqual(set(summary['secret_files']), expected)
            for path, digest in summary['secret_files'].items():
                self.assertEqual(hashlib.sha256((self.staging / label / path).read_bytes()).hexdigest(), digest)
            sealed = json.loads((self.keys / f'{label}.sealed.json').read_bytes())
            self.assertEqual({f['path']: f['sha256'] for f in sealed['files']}, summary['secret_files'])
            self.assertIn(f'dytallix-seal-{label} ', self.shown)
            host = next(h for h in self.plan['hosts'] if h['label'] == label)
            self.assertEqual(host['validator_public_key_base64'], summary['validator_public_key_base64'])
        # Only public records leave the staging directory.
        self.assertEqual(sorted(p.name for p in self.keys.iterdir()), sorted(
            ['PIN_PLAN.json', 'endpoint-1.channel-pin.json'] +
            [f'{label}.{kind}.json' for label in test_host_files.HOSTS for kind in ('keys', 'sealed')]))
        with self.assertRaises(host_keys.Invalid):
            host_keys.host_keys(self.plan, self.plan['hosts'][0], self.tools, self.staging, self.tmp, show=lambda _: None)

    def test_bundle_is_deterministic_and_complete(self):
        out, result = self.bundle('endpoint-1')
        again, second = self.bundle('endpoint-1', 'again.tar')
        self.assertEqual(result['sha256'], second['sha256'])
        self.assertEqual(out.with_name(out.name + '.sha256').read_text(), f'{result["sha256"]}  {out.name}\n')
        with tarfile.open(out) as tar:
            members = {m.name: m for m in tar.getmembers()}
        manifest = self.manifests['endpoint-1']
        for row in manifest['files']:
            self.assertIn(f'dytallix-host/files{row["path"]}', members)
        for binary in manifest['binaries']:
            self.assertEqual(members[f'dytallix-host/bin/{binary["name"]}'].mode, 0o755)
        self.assertEqual(members['dytallix-host/install.sh'].mode, 0o755)
        self.assertTrue(all(m.uid == 0 and m.mtime == 0 for m in members.values()))
        with self.assertRaises(host_bundle.Invalid):
            self.bundle('endpoint-1')  # never overwritten

    def test_bundle_refuses_mismatches(self):
        sealed_path = self.keys / 'validator-1.sealed.json'
        sealed = json.loads(sealed_path.read_bytes())
        wrong = dict(sealed, label='sentry-1')
        (self.tmp / 'wrong-label.json').write_text(json.dumps(wrong))
        missing = dict(sealed, files=sealed['files'][1:])
        (self.tmp / 'missing.json').write_text(json.dumps(missing))
        for name in ('wrong-label.json', 'missing.json'):
            with self.assertRaises(host_bundle.Invalid, msg=name):
                host_bundle.build(self.host_files / 'validator-1', self.network.dirs['release'], self.tmp / name,
                                  self.tmp / f'{name}.tar')
        binary = self.network.dirs['release'] / 'bin' / 'dytallix-pqc-engine'
        binary.write_bytes(b'another engine')
        with self.assertRaises(host_bundle.Invalid):
            self.bundle('validator-1')

    def test_install_verify_and_wipe(self):
        for label in ('validator-1', 'endpoint-1'):
            bundle, host, manifest, printed = self.install(label)
            self.assertEqual(hi.verify(bundle, host), [])
            self.assertIn(f'Type the seal code for {label}', '\n'.join(printed))
            release = manifest['release']
            for secret in manifest['secrets']:
                path = host.path(secret['path'])
                self.assertEqual(stat.S_IMODE(os.lstat(path).st_mode), 0o600)
                self.assertEqual(host.owner(secret['path']), (41001, 41001))
            self.assertEqual(stat.S_IMODE(os.lstat(host.path(f'/opt/dytallix/{release}/bin')).st_mode), 0o555)
            self.assertEqual(stat.S_IMODE(os.lstat(host.path(hi.HOME)).st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(os.lstat(host.path('/etc/nftables.d')).st_mode), 0o755)
            self.assertIn(hi.NFT_INCLUDE, host.path(hi.NFT_CONF).read_text())
            self.assertTrue(host.path(hi.JOURNALD_FILE).read_text().endswith('MaxRetentionSec=90d\n'))
            commands = [' '.join(c[:2]) for c in host.commands]
            self.assertLess(commands.index('groupadd --system'), commands.index('useradd --system'))
            self.assertIn('dytallix-node.service', host.enabled)
            # A second install refuses the existing one.
            with self.assertRaisesRegex(hi.Refused, 'already has a Dytallix install'):
                hi.install(bundle, host, out=lambda _: None)
            # Wipe asks first, then removes everything but the account.
            with self.assertRaises(hi.Refused):
                hi.wipe(bundle, host, confirm=lambda _: 'yes', out=lambda _: None)
            self.assertTrue(host.path('/var/lib/dytallix').exists())
            hi.wipe(bundle, host, confirm=lambda _: f'wipe {label}', out=lambda _: None)
            self.assertEqual(hi.existing(host), [])
            self.assertFalse(host.table)
            self.assertTrue(host.user)
            # And the host takes a fresh install again, reusing the account.
            hi.install(bundle, host, out=lambda _: None)
            self.assertEqual(hi.verify(bundle, host), [])

    def test_verify_reports_changes(self):
        bundle, host, manifest, _ = self.install('sentry-1')
        service = next(r['path'] for r in manifest['files'] if r['path'].endswith('/service.json'))
        os.chmod(host.path(service), 0o644)
        host.path(service).write_bytes(b'{}')
        os.chmod(host.path(service), 0o444)
        binary = manifest['binaries'][0]['path']
        host.immutable.discard(str(host.path(binary)))
        host.chown(hi.HOME, 0, 0)
        problems = '\n'.join(hi.verify(bundle, host))
        self.assertIn(f'{service}: content differs', problems)
        self.assertIn(f'{binary}: not immutable', problems)
        self.assertIn(f'{hi.HOME}: owner', problems)

    def test_install_refuses_unsuitable_hosts(self):
        out, _ = self.bundle('validator-1')
        bundle = self.unpack(out)
        profiles = self.manifests['validator-1']['apparmor_profiles']
        cases = {
            'Ubuntu 24.04': lambda h: h.path('/etc/os-release').write_text('ID=ubuntu\nVERSION_ID="22.04"\n'),
            'AppArmor': lambda h: h.path('/sys/module/apparmor/parameters/enabled').write_text('N\n'),
            'ufw is enabled': lambda h: ufw_conf(h, 'yes'),
            'firewalld is active': lambda h: setattr(h, 'run', active_unit('firewalld', h.run)),
            'already has': lambda h: h.path('/etc/dytallix').mkdir(),
            'root-owned without group or other write': lambda h: os.chmod(h.path('/opt'), 0o777),
        }
        for index, (message, change) in enumerate(cases.items()):
            host = FakeHost(self.tmp / f'unsuitable-{index}', self.staging, profiles)
            change(host)
            with self.assertRaisesRegex(hi.Refused, message):
                hi.install(bundle, host, out=lambda _: None)
            self.assertFalse(host.user, f'{message}: changed the host before refusing')
        # A changed file in the unpacked bundle is refused too.
        target = next(bundle.glob('files/etc/dytallix/*/service.json'))
        target.write_bytes(b'{}')
        host = FakeHost(self.tmp / 'tampered', self.staging, profiles)
        with self.assertRaisesRegex(hi.Refused, 'not the manifest'):
            hi.install(bundle, host, out=lambda _: None)


class DisabledUfwTests(HostNetwork):
    def test_install_accepts_ufw_disabled_with_its_unit_still_active(self):
        # GitHub's runners and a host after `ufw disable`: the oneshot unit
        # stays active, but ufw no longer filters.
        out, _ = self.bundle('validator-1')
        bundle = self.unpack(out)
        host = FakeHost(self.tmp / 'ufw-disabled', self.staging, self.manifests['validator-1']['apparmor_profiles'])
        ufw_conf(host, 'no')
        host.run = active_unit('ufw', host.run)
        hi.install(bundle, host, out=lambda _: None)
        self.assertEqual(hi.verify(bundle, host), [])


class ReleaseSwitchTests(HostNetwork):
    """A second release of the same chain, staged and switched to."""

    def second_release(self):
        release2 = self.tmp / 'release-2'
        (release2 / 'bin').mkdir(parents=True)
        record = json.loads((self.network.dirs['release'] / 'BUILD_RECORD.json').read_bytes())
        for name in record['members']:
            raw = (self.network.dirs['release'] / 'bin' / name).read_bytes()
            if name == 'dytallix-pqc-engine':
                raw = b'engine, second release'
                record['members'][name] = {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest(),
                                           'sha512': hashlib.sha512(raw).hexdigest()}
            (release2 / 'bin' / name).write_bytes(raw)
        (release2 / 'BUILD_RECORD.json').write_text(json.dumps(record))
        value = release_manifest.manifest(release_manifest.load_record(release2 / 'BUILD_RECORD.json'),
                                          test_host_files.CHAIN_ID, self.network.dirs['chain'] / 'native-genesis.json')
        (release2 / 'RELEASE_MANIFEST.json').write_bytes(release_manifest.encode(value))
        generated = host_files.generate(release2, self.network.dirs['chain'], self.network.dirs['hosts'], self.plan,
                                        self.keys, self.network.values, self.network.setup)
        host_files.write(self.tmp / 'host-files-2', generated)
        out = self.tmp / 'validator-1.release-2.tar'
        host_bundle.build(self.tmp / 'host-files-2' / 'validator-1', release2, self.keys / 'validator-1.sealed.json', out)
        return self.unpack(out), generated['validator-1'][1]

    def test_stage_then_switch_at_the_halt(self):
        bundle, host, manifest, _ = self.install('validator-1')
        old = manifest['release']
        host.active.add('dytallix-node.service')
        # The validator signs: its state changes, and verify accepts that.
        host.path(hi.SIGNING_STATE).write_bytes(b'{"height":"7"}')
        self.assertEqual(hi.verify(bundle, host), [])
        self.assertIn(f'{hi.SIGNING_STATE}: content differs', hi.verify(bundle, host, fresh=True))
        bundle2, manifest2 = self.second_release()
        new = manifest2['release']
        self.assertNotEqual(new, old)
        hi.stage(bundle2, host, out=lambda _: None)
        self.assertEqual(hi.active_release(host), old)
        self.assertTrue(host.path(f'/opt/dytallix/{old}/bin').exists())
        engine = str(host.path(f'/opt/dytallix/{new}/bin/dytallix-pqc-engine'))
        self.assertIn(engine, host.immutable)
        staged_unit = host.path(f'/etc/dytallix/{new}/next/dytallix-node.service').read_text()
        self.assertIn(f'/opt/dytallix/{new}/bin/', staged_unit)
        # Not while the old release runs.
        with self.assertRaisesRegex(hi.Refused, 'switch only once'):
            hi.switch(bundle2, host, out=lambda _: None)
        host.active.discard('dytallix-node.service')
        hi.switch(bundle2, host, out=lambda _: None)
        self.assertEqual(hi.active_release(host), new)
        self.assertIn('dytallix-node.service', host.active)
        self.assertEqual(hi.verify(bundle2, host), [])
        self.assertEqual(host.loaded, set(manifest2['apparmor_profiles']))  # the old release's are unloaded
        self.assertTrue(host.path(f'/opt/dytallix/{old}/bin').exists())  # kept until the next stage
        with self.assertRaisesRegex(hi.Refused, 'already active'):
            hi.stage(bundle2, host, out=lambda _: None)
        # Staging again (here the first release) removes all but the active one.
        hi.stage(bundle, host, out=lambda _: None)
        self.assertEqual(sorted(p.name for p in host.path('/opt/dytallix').iterdir()), sorted([old, new]))

    def test_stage_refuses_another_host_or_changed_home(self):
        _, host, _, _ = self.install('validator-1')
        sentry = self.unpack(self.bundle('sentry-1')[0])
        with self.assertRaisesRegex(hi.Refused, 'another host or chain'):
            hi.stage(sentry, host, out=lambda _: None)
        bundle2, _ = self.second_release()
        target = next(bundle2.glob('files/var/lib/dytallix/node/config/config.toml'))
        manifest = json.loads((bundle2 / 'INSTALL_MANIFEST.json').read_bytes())
        target.write_bytes(b'moniker = "changed"\n')
        row = next(r for r in manifest['files'] if r['path'].endswith('/config/config.toml'))
        row['sha256'], row['bytes'] = hashlib.sha256(target.read_bytes()).hexdigest(), target.stat().st_size
        (bundle2 / 'INSTALL_MANIFEST.json').write_text(json.dumps(manifest))
        with self.assertRaisesRegex(hi.Refused, 'never changes the node home'):
            hi.stage(bundle2, host, out=lambda _: None)


class RestoreTests(HostNetwork):
    """A freshly installed host restored from an off-host copy (R5)."""
    READY = json.dumps({'started': True, 'engine_readiness': {'block_height': 31, 'application_height': 31}})

    def restore_host(self, label, height=20):
        bundle, host, manifest, _ = self.install(label)
        host.copy_metadata = {'chain_id': manifest['chain_id'], 'height': height}
        copy = self.tmp / f'{label}-copy.bin'
        copy.write_bytes(b'encrypted copy')
        return bundle, host, manifest, copy

    def restore(self, bundle, host, copy, **options):
        printed = []
        report = hi.restore(bundle, host, copy, out=printed.append, sleep=lambda _: None, clock=lambda: 1000,
                            **options)
        return report, printed

    def test_restore_opens_the_copy_starts_once_and_cleans_up(self):
        bundle, host, manifest, copy = self.restore_host('sentry-1')
        seen = {}
        run = host.run

        def at_start(args, check=True, interactive=False):
            if args[:2] == ['systemctl', 'start']:
                drop_in = host.path(hi.RESTORE_DROP_IN).read_text()
                seen['drop_in'] = drop_in
                seen['modes'] = [oct(host.mode(p)) for p in (hi.RESTORE_COPY, f'{hi.RESTORE_COPY}/metadata.json',
                                                             f'{hi.RESTORE_COPY}/light-blocks')]
                host.journal = 'not json\n' + self.READY + '\n'
            return run(args, check=check, interactive=interactive)
        host.run = at_start
        report, printed = self.restore(bundle, host, copy)
        self.assertEqual(report['engine_readiness']['block_height'], 31)
        # The backup code is typed; the copy is opened into the fixed directory.
        opened = next(c for c in host.commands if c[1:2] == ['backup-open'])
        self.assertEqual(opened[opened.index('-paper') + 1], '-')
        self.assertEqual(opened[opened.index('-out') + 1], str(host.path(hi.RESTORE_COPY)))
        # One start with --restore-snapshot, through a drop-in that resets ExecStart.
        lines = seen['drop_in'].splitlines()
        self.assertEqual(lines[:2], ['[Service]', 'ExecStart='])
        self.assertTrue(lines[2].startswith(f'ExecStart=/opt/dytallix/{manifest["release"]}/bin/'))
        self.assertTrue(lines[2].endswith(f' --restore-snapshot {hi.RESTORE_COPY}'))
        self.assertEqual(seen['modes'], ['0o755', '0o644', '0o755'])
        # Afterwards: the drop-in and the opened copy are gone, the node runs.
        self.assertFalse(os.path.lexists(host.path(hi.RESTORE_DROP_IN)))
        self.assertFalse(os.path.lexists(host.path(hi.RESTORE_COPY)))
        self.assertTrue(os.path.isdir(host.path(hi.RESTORE)))
        self.assertEqual([c for c in host.commands if c == ['systemctl', 'daemon-reload']][-2:],
                         [['systemctl', 'daemon-reload']] * 2)
        self.assertIn(f'{hi.UNIT}.service', host.active)
        self.assertIn('at height 20', printed[-1])
        self.assertIn('caught up to height 31', printed[-1])

    def test_restore_refuses_unsafe_starts(self):
        # The validator only with --accept-history-loss.
        bundle, host, _, copy = self.restore_host('validator-1')
        with self.assertRaisesRegex(hi.Refused, 'accept-history-loss'):
            self.restore(bundle, host, copy)
        host.journal = self.READY
        self.restore(bundle, host, copy, accept_history_loss=True)
        # A running node, a node with state, a copy of another chain, a leftover copy.
        bundle, host, _, copy = self.restore_host('endpoint-1')
        host.active.add(f'{hi.UNIT}.service')
        with self.assertRaisesRegex(hi.Refused, 'stop the node'):
            self.restore(bundle, host, copy)
        host.active.clear()
        host.path(f'{hi.HOME}/data/blockstore.db').mkdir()
        with self.assertRaisesRegex(hi.Refused, 'freshly installed'):
            self.restore(bundle, host, copy)
        os.rmdir(host.path(f'{hi.HOME}/data/blockstore.db'))
        host.copy_metadata = {'chain_id': 'another-chain', 'height': 20}
        with self.assertRaisesRegex(hi.Refused, 'not of'):
            self.restore(bundle, host, copy)
        with self.assertRaisesRegex(hi.Refused, 'left from an earlier restore'):
            self.restore(bundle, host, copy)
        host.remove_tree(hi.RESTORE_COPY)
        # A start that stops without restoring: the drop-in goes, the copy stays for diagnosis.
        host.copy_metadata['chain_id'] = self.manifests['endpoint-1']['chain_id']
        run = host.run

        def stopped(args, check=True, interactive=False):
            result = run(args, check=check, interactive=interactive)
            if args[:2] == ['systemctl', 'start']:
                host.active.discard(args[2])
            return result
        host.run = stopped
        with self.assertRaisesRegex(hi.Refused, 'restore start stopped'):
            self.restore(bundle, host, copy)
        self.assertFalse(os.path.lexists(host.path(hi.RESTORE_DROP_IN)))
        # A start that never reports within the timeout is stopped.
        host.run = run
        host.remove_tree(hi.RESTORE_COPY)
        ticks = iter(range(1000, 10**6, 10**4))
        with self.assertRaisesRegex(hi.Refused, 'within'):
            hi.restore(bundle, host, copy, out=lambda _: None, sleep=lambda _: None, clock=lambda: next(ticks),
                       timeout=60)
        self.assertIn(['systemctl', 'stop', f'{hi.UNIT}.service'], host.commands)
        self.assertNotIn(f'{hi.UNIT}.service', host.active)
        # Ready, but still catching up past the copy: it waits, then says so.
        host.remove_tree(hi.RESTORE_COPY)
        host.journal, host.synced_height = self.READY, 12
        ticks = iter(range(1000, 10**6, 10**4))
        with self.assertRaisesRegex(hi.Refused, 'has not caught up'):
            hi.restore(bundle, host, copy, out=lambda _: None, sleep=lambda _: None, clock=lambda: next(ticks),
                       timeout=60)
        self.assertEqual(hi.synced('not json'), (0, True))


def active_unit(unit, run):
    def wrapped(args, check=True, interactive=False):
        if args[:3] == ['systemctl', 'is-active', unit]:
            return SimpleNamespace(returncode=0, stdout='active\n', stderr='')
        return run(args, check=check, interactive=interactive)
    return wrapped


def ufw_conf(host, enabled):
    host.path(hi.UFW_CONF).parent.mkdir(parents=True, exist_ok=True)
    host.path(hi.UFW_CONF).write_text(f'# /etc/ufw/ufw.conf\nENABLED={enabled}\nLOGLEVEL=low\n')


if __name__ == '__main__':
    unittest.main()


class MonitorHostTests(HostNetwork):
    """The monitor job on every host (monitoring v1, M4): its files, unit and
    timer from the bundle, and its settings from the founder's own media."""

    SETTINGS = {'schema': 'dytallix.monitor-settings.v1', 'label': 'validator-1',
                'heartbeat_url': 'https://beat.example/ping/v1', 'alert_url': 'https://alerts.example/hook',
                'escalation_url': None}

    def test_every_host_gets_the_job_with_the_approved_values(self):
        values = {r['path']: r['approved'] for r in self.network.values['values'] if r['status'] == 'APPROVED'}
        for label, manifest in self.manifests.items():
            etc = f'/etc/dytallix/{manifest["release"]}'
            files = {r['path'] for r in manifest['files']}
            self.assertTrue({f'{etc}/monitor.json', f'{etc}/monitor.py', '/etc/systemd/system/dytallix-monitor.service',
                             '/etc/systemd/system/dytallix-monitor.timer'} <= files, label)
            self.assertIn('dytallix-monitor.timer', manifest['timers'])
            directories = {d['path']: (d['owner'], d['mode']) for d in manifest['directories']}
            self.assertEqual(directories['/etc/dytallix-monitor'], ('root', '0700'))
            self.assertEqual(directories['/var/lib/dytallix-monitor'], ('root', '0700'))
            # The settings are never in the bundle or the sealed keys.
            self.assertFalse(any('monitor' in s['path'] for s in manifest['secrets']))
            config = json.loads((self.host_files / label / etc.lstrip('/') / 'monitor.json').read_bytes())
            self.assertEqual((config['label'], config['role']), (label, manifest['role']))
            self.assertEqual(config['thresholds'], {name: values['monitor.' + name] for name in host_files.MONITOR_VALUES})
            self.assertEqual(config['emission_ceiling_udrt_per_block'], 1_000_000_000)
            self.assertEqual(config['status_url'] is not None, manifest['role'] == 'endpoint')
            self.assertIsNone(config['backup_state'])
        unit = (self.host_files / 'validator-1' / 'etc/systemd/system/dytallix-monitor.service').read_text()
        self.assertIn('CapabilityBoundingSet=\n', unit)
        self.assertIn('ReadWritePaths=/var/lib/dytallix-monitor\n', unit)
        self.assertIn('ProtectSystem=strict', unit)
        timer = (self.host_files / 'validator-1' / 'etc/systemd/system/dytallix-monitor.timer').read_text()
        self.assertIn('OnCalendar=*-*-* *:*:00', timer)
        endpoint = self.manifests['endpoint-1']
        config = json.loads((self.host_files / 'endpoint-1' / f'etc/dytallix/{endpoint["release"]}/monitor.json')
                            .read_bytes())
        status = next(h['status'] for h in self.plan['hosts'] if h['label'] == 'endpoint-1')
        self.assertEqual(config['status_url'], f'http://{status}/status')

    def test_an_unapproved_threshold_is_refused(self):
        values = json.loads(json.dumps(self.network.values))
        next(r for r in values['values'] if r['path'] == 'monitor.halt_seconds')['status'] = 'OPEN'
        with self.assertRaisesRegex(host_files.Invalid, 'monitor.halt_seconds is not approved'):
            host_files.generate(self.network.dirs['release'], self.network.dirs['chain'], self.network.dirs['hosts'],
                                self.plan, self.keys, values, self.network.setup)

    def settings_file(self, **change):
        path = self.tmp / 'webhooks.json'
        path.write_text(json.dumps(dict(self.SETTINGS, **change)))
        return path

    def test_install_settings_replace_and_wipe(self):
        bundle, host, manifest, printed = self.install('validator-1')
        self.assertIn('dytallix-monitor.timer', host.enabled)
        self.assertIn('./monitor-settings.sh', '\n'.join(printed))
        self.assertEqual(hi.verify(bundle, host), [])
        self.assertTrue((bundle / 'monitor-settings.sh').exists())
        shown = []
        self.assertTrue(hi.monitor_settings(bundle, host, self.settings_file(), out=shown.append))
        installed = host.path(hi.MONITOR_SETTINGS)
        self.assertEqual(stat.S_IMODE(os.lstat(installed).st_mode), 0o400)
        self.assertEqual(host.owner(hi.MONITOR_SETTINGS), (0, 0))
        self.assertEqual(json.loads(installed.read_bytes())['alert_url'], 'https://alerts.example/hook')
        self.assertIn(['systemctl', 'start', 'dytallix-monitor.service'], host.commands)
        self.assertNotIn('alerts.example', '\n'.join(shown), 'the URLs are not printed')
        self.assertIn('by the alerting service', '\n'.join(shown))
        # Replaced in one step, later.
        hi.monitor_settings(bundle, host, self.settings_file(alert_url='https://other.example/hook',
                                                             escalation_url='https://backup.example/x'),
                            out=lambda _: None)
        self.assertEqual(json.loads(installed.read_bytes())['alert_url'], 'https://other.example/hook')
        self.assertEqual(hi.verify(bundle, host), [], 'the settings are not part of the bundle check')
        hi.wipe(bundle, host, confirm=lambda _: 'wipe validator-1', out=lambda _: None)
        self.assertEqual(hi.existing(host), [])
        self.assertNotIn('dytallix-monitor.timer', host.enabled)

    def test_settings_refusals(self):
        bundle, host, _, _ = self.install('validator-1')
        cases = (({'label': 'sentry-1'}, 'for sentry-1'),
                 ({'alert_url': 'http://alerts.example/hook'}, 'https'),
                 ({'heartbeat_url': 'https://beat.example/a b'}, 'https'),
                 ({'heartbeat_url': 'https://beat.example/"x'}, 'https'),
                 ({'escalation_url': 'https://u:p@backup.example/'}, 'https'),
                 ({'extra': 1}, 'not monitor settings'),
                 ({'schema': 'other'}, 'not monitor settings'))
        for change, message in cases:
            with self.subTest(change=change):
                with self.assertRaisesRegex(hi.Refused, message):
                    hi.monitor_settings(bundle, host, self.settings_file(**change), out=lambda _: None)
        self.assertFalse(os.path.lexists(host.path(hi.MONITOR_SETTINGS)))
        (self.tmp / 'bad.json').write_text('{')
        with self.assertRaisesRegex(hi.Refused, 'not JSON'):
            hi.monitor_settings(bundle, host, self.tmp / 'bad.json', out=lambda _: None)
        # A stand-in receiver on the host may use plain HTTP.
        hi.monitor_settings(bundle, host, self.settings_file(alert_url='http://127.0.0.1:9100/alert'),
                            out=lambda _: None)


class BackupHostTests(HostNetwork):
    """The sentry's backup secrets and unit (disaster recovery v1)."""
    backup = True

    def test_only_the_sentry_seals_and_installs_backup_secrets(self):
        for label, summary in self.summaries.items():
            backup = {p for p in summary['secret_files'] if p.startswith('backup/')}
            self.assertEqual(backup, set(host_files.BACKUP_SECRETS) if label == 'sentry-1' else set())
        upload = self.staging / 'sentry-1' / 'backup' / 'upload.json'
        self.assertEqual(stat.S_IMODE(os.lstat(upload).st_mode), 0o600)
        manifest = self.manifests['sentry-1']
        self.assertEqual(manifest['timers'], ['dytallix-backup.timer', 'dytallix-monitor.timer'])
        self.assertEqual(self.manifests['validator-1']['timers'], ['dytallix-monitor.timer'])
        monitor = json.loads((self.host_files / 'sentry-1' / f'etc/dytallix/{manifest["release"]}/monitor.json')
                             .read_bytes())
        self.assertEqual(monitor['backup_state'], '/var/lib/dytallix-backup/last-uploaded')
        secrets = {s['path']: (s['owner'], s['mode']) for s in manifest['secrets']}
        self.assertEqual(secrets['/etc/dytallix-backup/code'], ('root', '0400'))
        self.assertEqual(secrets['/etc/dytallix-backup/upload.json'], ('root', '0400'))
        files = {r['path'] for r in manifest['files']}
        etc = f'/etc/dytallix/{manifest["release"]}'
        self.assertTrue({f'{etc}/backup.json', f'{etc}/backup.py', '/etc/systemd/system/dytallix-backup.service',
                         '/etc/systemd/system/dytallix-backup.timer'} <= files)
        config = json.loads((self.host_files / 'sentry-1' / etc.lstrip('/') / 'backup.json').read_bytes())
        self.assertEqual((config['code_file'], config['snapshots']), ('/etc/dytallix-backup/code', host_files.SNAPSHOTS))
        unit = (self.host_files / 'sentry-1' / 'etc/systemd/system/dytallix-backup.service').read_text()
        self.assertIn('CapabilityBoundingSet=CAP_DAC_READ_SEARCH', unit)
        self.assertIn('ReadWritePaths=/var/lib/dytallix-backup', unit)

    def test_install_places_the_secrets_outside_the_node_home(self):
        bundle, host, manifest, _ = self.install('sentry-1')
        self.assertEqual(hi.verify(bundle, host), [])
        for name in ('code', 'upload.json'):
            path = f'/etc/dytallix-backup/{name}'
            self.assertEqual(stat.S_IMODE(os.lstat(host.path(path)).st_mode), 0o400)
            self.assertEqual(host.owner(path), (0, 0))
        self.assertEqual(json.loads(host.path('/etc/dytallix-backup/upload.json').read_bytes()), UPLOAD)
        self.assertFalse(host.path(hi.HOME + '/backup').exists())
        self.assertFalse(host.path(hi.UNSEALED).exists(), 'the unsealed copy was left behind')
        self.assertIn('dytallix-backup.timer', host.enabled)
        hi.wipe(bundle, host, confirm=lambda _: 'wipe sentry-1', out=lambda _: None)
        self.assertEqual(hi.existing(host), [])
        self.assertNotIn('dytallix-backup.timer', host.enabled)

    def test_a_backup_secret_on_another_host_is_refused(self):
        key = json.loads((self.keys / 'validator-1.keys.json').read_bytes())
        key['secret_files']['backup/code'] = '00' * 32
        key['secret_files']['backup/upload.json'] = '00' * 32
        (self.keys / 'validator-1.keys.json').write_text(json.dumps(key))
        with self.assertRaisesRegex(host_files.Invalid, 'only the sentry'):
            host_files.generate(self.network.dirs['release'], self.network.dirs['chain'], self.network.dirs['hosts'],
                                self.plan, self.keys, self.network.values, self.network.setup)

