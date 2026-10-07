"""The host file generator on a synthetic three-host staging network: the
committed production rehearsal genesis, a release manifest from the release
tool, and synthetic host-config outputs and offline key summaries. No real
key, host or approval."""
import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
import host_files as h

HERE = Path(__file__).resolve().parent
MAINNET = HERE.parents[2]
sys.path.insert(0, str(MAINNET / 'release'))
import release_manifest  # noqa: E402

REHEARSAL = HERE / 'fixtures' / 'genesis-production-rehearsal'
CHAIN_ID = 'dytallix-staging-1'
BINARIES = ['consensus_stdio', 'dytallix-comet-bridge', 'dytallix-native-supervisor', 'dytallix-pqc-engine',
            'dytallix-pqc-http-adapter', 'dytallix-root-verify', 'dytallix-control', 'dytallix-host-config']
ROLES = {'consensus_bridge': 'dytallix-comet-bridge', 'consensus_engine': 'dytallix-pqc-engine',
         'consensus_stdio': 'consensus_stdio', 'control_verifier': 'dytallix-root-verify',
         'genesis_bootstrap_verifier': 'dytallix-root-verify', 'http_adapter': 'dytallix-pqc-http-adapter',
         'service_supervisor': 'dytallix-native-supervisor'}
HOSTS = {  # label: role, address, pins
    'validator-1': ('validator', '10.20.0.10', ['sentry-1']),
    'sentry-1': ('sentry', '10.20.0.11', ['endpoint-1', 'validator-1']),
    'endpoint-1': ('endpoint', '10.20.0.12', ['sentry-1']),
}


def key(seed):
    return base64.b64encode(hashlib.shake_256(seed.encode()).digest(1952)).decode()


def write(path, raw):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw if isinstance(raw, bytes) else json.dumps(raw, indent=2).encode())


FIXTURE = HERE / 'fixtures' / 'host-files-staging'
FIXTURE_NAMES = ('service.json', 'root-config.json', 'emergency-verifier.json', 'candidate.json')


def fixture_files(generated):
    """The generated configurations the supervisor's tests read, by host."""
    return {label: {name: files[f'/etc/dytallix/{manifest["release"]}/{name}'] for name in FIXTURE_NAMES}
            for label, (files, manifest) in generated.items()}


class HostFilesTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.dirs = {name: root / name for name in ('release', 'chain', 'hosts', 'keys')}
        # The release: a build record, and its manifest from the release tool.
        members = {name: {'bytes': 1000 + i, 'sha256': hashlib.sha256(name.encode()).hexdigest(),
                          'sha512': hashlib.sha512(name.encode()).hexdigest()} for i, name in enumerate(BINARIES)}
        record = {'schema': release_manifest.RECORD_SCHEMA, 'target': release_manifest.TARGET, 'linkage': 'static',
                  'migration_registry_sha256': '22' * 32, 'members': members, 'catalog_roles': ROLES}
        write(self.dirs['release'] / 'BUILD_RECORD.json', record)
        value = release_manifest.manifest(release_manifest.load_record(self.dirs['release'] / 'BUILD_RECORD.json'),
                                          CHAIN_ID, REHEARSAL / 'native-genesis.json')
        write(self.dirs['release'] / 'RELEASE_MANIFEST.json', release_manifest.encode(value))
        # The chain: the production rehearsal and synthetic root records.
        for name in ('application-config.json', 'native-genesis.json', 'genesis.json'):
            write(self.dirs['chain'] / name, (REHEARSAL / name).read_bytes())
        write(self.dirs['chain'] / 'root-policy.json', b'{"synthetic":"root signer policy"}')
        write(self.dirs['chain'] / 'root-signatures.json', b'{"synthetic":"root signatures"}')
        # The plan, host-config's outputs and the offline key summaries.
        self.plan = {'schema': 'dytallix.pin-plan.v1', 'chain_id': CHAIN_ID, 'hosts': []}
        bindings = {'hosts': []}
        for label, (role, ip, pins) in HOSTS.items():
            plan_host = {'label': label, 'operator': 'founder', 'role': role, 'home': h.HOME,
                         'p2p': f'{ip}:26656', 'channel': f'{ip}:26670' if role == 'endpoint' else None,
                         'status': f'{ip}:8080' if role == 'endpoint' else None,
                         'peer_public_key_base64': key('peer-' + label),
                         'validator_public_key_base64': key('validator-' + label), 'pins': pins, 'state_sync': None}
            self.plan['hosts'].append(plan_host)
            transport = json.dumps({'version': 1, 'profile': 'dytallix-pqc-production-v1', 'network': CHAIN_ID,
                                    'local_public_key_base64': plan_host['peer_public_key_base64'],
                                    'peers': [{'id': '%040x' % i, 'public_key_base64': key('peer-' + p),
                                               'address': HOSTS[p][1] + ':26656'} for i, p in enumerate(pins)],
                                    'handshake_timeout_ms': 5000}, separators=(',', ':')).encode()
            out = self.dirs['hosts'] / 'hosts' / label
            write(out / 'config.toml', f'moniker = "{label}"\n'.encode())
            write(out / 'pqc_transport.json', transport)
            write(out / 'binding.json', json.dumps({'synthetic_binding': label}).encode())
            public = [plan_host['channel'], plan_host['status']] if role == 'endpoint' else []
            bindings['hosts'].append({'label': label, 'firewall': {
                'transport_sha256': hashlib.sha256(transport).hexdigest(), 'p2p_listen': plan_host['p2p'],
                'public_listeners': public}})
            secrets = {'config/pqc_peer_seed.bin': 'a' * 64, 'config/priv_validator_key.json': 'b' * 64,
                       'data/priv_validator_state.json': 'c' * 64}
            if role == 'endpoint':
                secrets['config/client_channel_seed.bin'] = 'd' * 64
                write(self.dirs['keys'] / f'{label}.channel-pin.json', b'{"synthetic":"channel pin"}')
            write(self.dirs['keys'] / f'{label}.keys.json', {
                'schema': h.KEYS_SCHEMA, 'label': label, 'role': role,
                'peer_public_key_base64': plan_host['peer_public_key_base64'],
                'validator_public_key_base64': plan_host['validator_public_key_base64'], 'secret_files': secrets})
        write(self.dirs['hosts'] / 'PIN_PLAN_BINDINGS.json', bindings)
        self.values = json.loads((MAINNET / 'launch' / 'E05_VALUES.json').read_text())
        self.setup = json.loads((MAINNET / 'launch' / 'hosts' / 'SETUP_VALUES.json').read_text())

    def generate(self, plan=None):
        return h.generate(self.dirs['release'], self.dirs['chain'], self.dirs['hosts'], plan or self.plan,
                          self.dirs['keys'], self.values, self.setup)

    def test_every_host_gets_consistent_files(self):
        generated = self.generate()
        self.assertEqual(set(generated), set(HOSTS))
        catalog = json.loads((self.dirs['release'] / 'RELEASE_MANIFEST.json').read_bytes())
        helper = next(m for m in catalog['members'] if m['id'] == 'dytallix-root-verify')
        for label, (files, manifest) in generated.items():
            role = HOSTS[label][0]
            etc = f'/etc/dytallix/{manifest["release"]}'
            service = json.loads(files[f'{etc}/service.json'])
            # Every pinned public file is the installed bytes.
            pins = [service[k] for k in ('consensus_config', 'application_genesis', 'root_config', 'emergency_verifier_config',
                                         'candidate_config', 'process_admission', 'binding')]
            pins += service['root_public_inputs'] + [p for p in service['engine_inputs'] if p['path'] in files]
            for pin in pins:
                self.assertEqual(pin['sha256'], hashlib.sha256(files[pin['path']]).hexdigest(), pin['path'])
                self.assertLessEqual(len(files[pin['path']]), pin['max_bytes'])
            self.assertEqual(sorted(p['path'] for p in service['engine_inputs']),
                             sorted(f'{h.HOME}/config/{n}' for n in ('config.toml', 'genesis.json', 'pqc_peer_seed.bin',
                                                                     'pqc_transport.json', 'priv_validator_key.json')))
            self.assertEqual((service['mode'], service['role'], service['home']), ('production-native', role, h.HOME))
            # The root helper is the catalog's, with the admission's identity.
            root = json.loads(files[f'{etc}/root-config.json'])
            admission = json.loads(files[f'{etc}/admission.json'])
            self.assertEqual((root['helper_sha256'], root['helper_execution']['helper_sha512']),
                             (helper['sha256'], helper['sha512']))
            for name in ('supervisor_label', 'application_owner_label', 'workload_label', 'helper_label', 'uid', 'gid'):
                self.assertEqual(root['helper_execution']['owner_security'][name], admission[name])
            self.assertEqual(root['engine_genesis_path'], f'{h.HOME}/config/genesis.json')
            self.assertEqual({p['path'] for p in service['root_public_inputs']},
                             {root['policy_path'], root['signatures_path']})
            # Roles run what the supervisor requires of them.
            self.assertEqual('adapter_listen' in service and 'adapter_channel' in service, role == 'endpoint')
            self.assertEqual(service['block_history'], 'archive' if role == 'sentry' else 'window')
            self.assertEqual('snapshots' in service, role == 'sentry')
            # The unit runs the catalog's supervisor as the service account.
            unit = files[f'/etc/systemd/system/{h.UNIT}.service'].decode()
            self.assertIn(f'ExecStart=/opt/dytallix/{manifest["release"]}/bin/dytallix-native-supervisor '
                          f'--service-config {etc}/service.json', unit)
            self.assertIn('User=41001', unit)
            self.assertIn('Restart=no', unit)
            self.assertIn('AppArmorProfile=' + admission['supervisor_label'], unit)
            self.assertIn(f'MemoryMax={6 << 30}', unit)
            # The profiles and the firewall table.
            profiles = files[f'/etc/apparmor.d/{h.UNIT}'].decode()
            for label_name in manifest['apparmor_profiles']:
                self.assertIn(f'profile {label_name} {{', profiles)
            rules = files['/etc/nftables.d/dytallix.nft'].decode()
            self.assertIn(f'ip daddr {HOSTS[label][1]} tcp dport 26656 accept', rules)
            if role == 'endpoint':
                self.assertIn(f'ip daddr {HOSTS[label][1]} tcp dport 26670 accept', rules)
                self.assertIn(f'ip daddr {HOSTS[label][1]} tcp dport 8080 accept', rules)
            # The manifest: every file, the secrets the sealed keys provide, the binaries.
            self.assertEqual({r['path'] for r in manifest['files']}, set(files))
            for row in manifest['files']:
                self.assertEqual(row['sha256'], hashlib.sha256(files[row['path']]).hexdigest())
                self.assertEqual(row['owner'], h.USER if row['path'].startswith(h.HOME + '/') else 'root')
            self.assertEqual(len(manifest['secrets']), 4 if role == 'endpoint' else 3)
            self.assertEqual({b['name'] for b in manifest['binaries']}, set(BINARIES))

    def test_the_committed_fixture_is_current(self):
        # The supervisor's tests parse these with its own types (config.rs).
        for label, files in fixture_files(self.generate()).items():
            for name, raw in files.items():
                self.assertEqual((FIXTURE / label / name).read_bytes(), raw, f'{label}/{name}')

    def test_a_state_sync_host_reads_its_light_blocks(self):
        plan = copy.deepcopy(self.plan)
        endpoint = next(h for h in plan['hosts'] if h['role'] == 'endpoint')
        endpoint['state_sync'] = {'trust_height': 1, 'trust_hash': '00' * 32}
        generated = self.generate(plan)
        for label, (files, manifest) in generated.items():
            profile = files['/etc/apparmor.d/dytallix-node'].decode()
            service = json.loads(files[f'/etc/dytallix/{manifest["release"]}/service.json'])
            if label == 'endpoint-1':
                self.assertEqual(service['state_sync'], {'light_blocks': [h.LIGHT_BLOCKS]})
                # Every role may list and read the export, none may write or run it.
                self.assertEqual(profile.count(f'  {h.LIGHT_BLOCKS}/** r,'), 4)
                self.assertEqual(profile.count(f'  {h.LIGHT_BLOCKS}/ r,'), 4)
            else:
                self.assertNotIn('state_sync', service)
                self.assertNotIn(h.LIGHT_BLOCKS, profile)

    def test_written_out(self):
        out = Path(self.tmp.name) / 'out'
        h.write(out, self.generate())
        for label in HOSTS:
            manifest = json.loads((out / label / 'INSTALL_MANIFEST.json').read_text())
            for row in manifest['files']:
                self.assertEqual(hashlib.sha256((out / label / row['path'].lstrip('/')).read_bytes()).hexdigest(), row['sha256'])
        with self.assertRaises(h.Invalid):
            h.write(out, self.generate())

    def test_refusals(self):
        def refused(change, message):
            plan = copy.deepcopy(self.plan)
            change(plan)
            with self.assertRaises(h.Invalid) as caught:
                self.generate(plan)
            self.assertIn(message, str(caught.exception))
        refused(lambda p: p['hosts'][0].update(home='/srv/node'), 'plan home')
        refused(lambda p: p['hosts'][1].update(peer_public_key_base64=key('other')), 'differs from the plan')
        # The transport the bindings name must be the one installed.
        transport = self.dirs['hosts'] / 'hosts' / 'sentry-1' / 'pqc_transport.json'
        transport.write_bytes(transport.read_bytes().replace(b'5000', b'6000'))
        refused(lambda p: None, 'another transport file')

    def test_setup_and_e05_agree(self):
        for row in self.values['values']:
            if row['name'] == 'emergency_verifier_timeout_ms':
                row.update(status='APPROVED', approved=self.setup['emergency_verifier_timeout_ms'] + 1)
        with self.assertRaises(h.Invalid):
            self.generate()

    def test_observed_helper_bound_within_helper_bound(self):
        # The node refuses an observed file bound above the helper bound;
        # the 6 October values had 32 MiB against 16 MiB (H4).
        self.setup['root_helper_observation']['max_file_bytes'] = self.setup['root']['max_helper_bytes'] + 1
        with self.assertRaisesRegex(h.Invalid, 'max_file_bytes exceeds the root max_helper_bytes'):
            self.generate()

    def test_secrets_and_release_must_match(self):
        keys = self.dirs['keys'] / 'endpoint-1.keys.json'
        summary = json.loads(keys.read_text())
        del summary['secret_files']['config/client_channel_seed.bin']
        keys.write_text(json.dumps(summary))
        with self.assertRaises(h.Invalid):
            self.generate()
        self.setUp()
        record = json.loads((self.dirs['release'] / 'BUILD_RECORD.json').read_text())
        record['members']['dytallix-root-verify']['sha256'] = '0' * 64
        (self.dirs['release'] / 'BUILD_RECORD.json').write_text(json.dumps(record))
        with self.assertRaises(h.Invalid):
            self.generate()


if __name__ == '__main__':
    unittest.main(verbosity=2)
