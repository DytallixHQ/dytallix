"""The release manifest writer (E06). Builds nothing; the record is synthetic."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import release_manifest as m

HERE = Path(__file__).resolve().parent
MAINNET = HERE.parent
# A production catalog written by the release runtime's serializer, and the
# writer's output for RECORD, which the release runtime's validator accepts
# (component_candidate_tests.rs).
SERDE_CATALOG = MAINNET/'node/tools/native-execution-policy/testdata/abi4-service/catalog.json'
FIXTURE = MAINNET/'node/crates/release-runtime/testdata/production_release_manifest.json'
NATIVE_GENESIS = b'{"synthetic":"native genesis for the writer fixture"}'


def member(n):
    return {'bytes': 1000 + n, 'sha256': f'{n:02x}' * 32, 'sha512': f'{n:02x}' * 64}


RECORD = {
    'schema': m.RECORD_SCHEMA,
    'target': {'os': 'linux', 'arch': 'x86_64', 'abi': 'gnu'},
    'linkage': 'static',
    'migration_registry_sha256': '22' * 32,
    'members': {name: member(n) for n, name in enumerate([
        'consensus_stdio', 'dytallix', 'dytallix-comet-bridge', 'dytallix-native-supervisor',
        'dytallix-pqc-engine', 'dytallix-pqc-http-adapter', 'dytallix-root-verify'], start=1)},
    'catalog_roles': {
        'consensus_bridge': 'dytallix-comet-bridge', 'consensus_engine': 'dytallix-pqc-engine',
        'consensus_stdio': 'consensus_stdio', 'control_verifier': 'dytallix-root-verify',
        'genesis_bootstrap_verifier': 'dytallix-root-verify', 'http_adapter': 'dytallix-pqc-http-adapter',
        'service_supervisor': 'dytallix-native-supervisor'},
}


class WriterTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.genesis = Path(self.tmp.name)/'native-genesis.json'
        self.genesis.write_bytes(NATIVE_GENESIS)

    def record(self, data=RECORD):
        path = Path(self.tmp.name)/'BUILD_RECORD.json'
        path.write_text(json.dumps(data))
        return m.load_record(path)

    def test_the_encoding_is_serdes(self):
        raw = SERDE_CATALOG.read_bytes().rstrip(b'\n')
        catalog = json.loads(raw)
        self.assertEqual(m.encode(catalog), raw)
        # The writer emits ManifestV2's fields in the same order.
        value = m.manifest(self.record(), 'dytallix-mainnet-1', self.genesis)
        self.assertEqual(list(value), list(catalog))
        for part in ('members', 'roles', 'runtime_profiles'):
            self.assertEqual(list(value[part][0]), list(catalog[part][0]), part)
        self.assertEqual(list(value['target']), list(catalog['target']))

    def test_the_fixture_is_the_writers_output(self):
        raw = m.encode(m.manifest(self.record(), 'dytallix-mainnet-1', self.genesis))
        self.assertEqual(FIXTURE.read_bytes(), raw)

    def test_the_manifest(self):
        value = m.manifest(self.record(), 'dytallix-mainnet-1', self.genesis)
        self.assertEqual(value['service_profile'], 'production-linux-native-service-v1')
        self.assertEqual(value['app_genesis_sha256'], hashlib.sha256(NATIVE_GENESIS).hexdigest())
        self.assertEqual(value['migration_registry_sha256'], '22' * 32)
        ids = [x['id'] for x in value['members']]
        # Catalog binaries only, once each: the wallet is not a role.
        self.assertEqual(ids, sorted(set(RECORD['catalog_roles'].values())))
        self.assertTrue(all(x['kind'] == 'executable' for x in value['members']))
        self.assertEqual([r['role'] for r in value['roles']], sorted(RECORD['catalog_roles']))
        self.assertTrue(all(p['member_ids'] == [] and p['interpreted_member_ids'] == []
                            for p in value['runtime_profiles']))
        raw = m.encode(value)
        summary = m.summary(raw, value)
        self.assertEqual(summary['release_sha512'], hashlib.sha512(raw).hexdigest())
        self.assertEqual(summary['bootstrap_verifier']['sha256'], RECORD['members']['dytallix-root-verify']['sha256'])

    def test_refusals(self):
        for change, message in (
            (lambda d: d.update(schema='dytallix.release-build-record.v0'), 'schema'),
            (lambda d: d.update(linkage='dynamic'), 'static'),
            (lambda d: d['target'].update(abi='musl'), 'Linux x86_64'),
            (lambda d: d.update(migration_registry_sha256='22'), 'hex'),
            (lambda d: d['catalog_roles'].pop('http_adapter'), 'every production role'),
            (lambda d: d['members'].pop('dytallix-pqc-engine'), 'not in the build record'),
        ):
            data = copy.deepcopy(RECORD); change(data)
            with self.assertRaises(ValueError) as caught: self.record(data)
            self.assertIn(message, str(caught.exception))
        data = copy.deepcopy(RECORD)
        data['members']['dytallix-pqc-engine'] = data['members']['dytallix-comet-bridge']
        with self.assertRaises(ValueError): m.manifest(self.record(data), 'dytallix-mainnet-1', self.genesis)
        with self.assertRaises(ValueError): m.manifest(self.record(), 'bad chain', self.genesis)

    def test_the_default_chain_is_the_network_identity(self):
        self.assertEqual(m.default_chain_id(), 'dytallix-mainnet-1')


if __name__ == '__main__': unittest.main(verbosity=2)
