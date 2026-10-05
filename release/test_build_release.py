"""The release build driver's checks, sums and SBOM (E06). No build runs here."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
import unittest.mock
import build_release as r

HERE = Path(__file__).resolve().parent
MAINNET = HERE.parent


class ReleaseSetTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads((HERE/'RELEASE_SET.json').read_text())
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def load(self, data):
        path = Path(self.tmp.name)/'set.json'
        path.write_text(json.dumps(data))
        return r.load_set(path)

    def test_the_committed_set(self):
        data, names = r.load_set(HERE/'RELEASE_SET.json')
        # Every production catalog role (release-runtime component_candidate.rs).
        self.assertEqual(set(data['catalog_roles']), {
            'consensus_stdio', 'consensus_bridge', 'consensus_engine', 'genesis_bootstrap_verifier',
            'control_verifier', 'service_supervisor', 'http_adapter'})
        # Both verifier roles are the one root helper (native-supervisor service.rs).
        self.assertEqual(data['catalog_roles']['genesis_bootstrap_verifier'], data['catalog_roles']['control_verifier'])
        for tool in ('dytallix', 'dytallix-root-sign', 'dytallix-host-config', 'dytallix-peer-seed', 'dytallix-channel-key'):
            self.assertIn(tool, names)
        # Each build's sources exist in the tree.
        for build in data['builds']:
            if build['kind'] == 'cargo':
                self.assertTrue((MAINNET/build['workspace']/'Cargo.lock').is_file(), build['id'])
            else:
                for package in build['packages']:
                    self.assertTrue((MAINNET/build['module']/package).is_dir(), package)
        # Production builds of every node member that has one.
        self.assertIn('production', data['builds'][0]['args'])
        self.assertEqual(next(b for b in data['builds'] if b['id'] == 'engine')['tags'], ['production'])

    def test_the_set_is_checked(self):
        for change, message in (
            (lambda d: d.update(schema='dytallix.release-set.v0'), 'schema'),
            (lambda d: d['target'].update(arch='aarch64'), 'x86_64'),
            (lambda d: d['builds'][0]['bins'].append('dytallix'), 'distinct'),
            (lambda d: d['builds'][0]['bins'].append('../escape'), 'distinct'),
            (lambda d: d['catalog_roles'].update(http_adapter='missing-binary'), 'unbuilt'),
            (lambda d: d['builds'][0].update(kind='make'), 'unknown build kind'),
        ):
            data = copy.deepcopy(self.data); change(data)
            with self.assertRaises(ValueError) as caught: self.load(data)
            self.assertIn(message, str(caught.exception))


class RustPinTests(unittest.TestCase):
    def test_every_workspace_pins_the_builder_toolchain(self):
        data, _ = r.load_set(HERE/'RELEASE_SET.json')
        dockerfile = (HERE/'builder'/'Dockerfile').read_text()
        pinned = next(line.split('=')[1].split()[0] for line in dockerfile.splitlines() if 'RUSTUP_TOOLCHAIN=' in line)
        with unittest.mock.patch.dict(r.os.environ, {'RUSTUP_TOOLCHAIN': pinned}):
            r.check_rust_pin(MAINNET, data)
        with unittest.mock.patch.dict(r.os.environ, {'RUSTUP_TOOLCHAIN': '1.87.0-x86_64-unknown-linux-gnu'}):
            with self.assertRaises(ValueError): r.check_rust_pin(MAINNET, data)


class SumsTests(unittest.TestCase):
    def test_sums_are_sorted_and_compare_exactly(self):
        members = {'b': {'sha256': 'b' * 64, 'sha512': 'b' * 128}, 'a': {'sha256': 'a' * 64, 'sha512': 'a' * 128}}
        text = r.sums(members, 'sha256')
        self.assertEqual(text, f'{"a" * 64}  bin/a\n{"b" * 64}  bin/b\n')
        self.assertEqual(r.parse_sums(r.sums(members, 'sha512')), {'bin/a': 'a' * 128, 'bin/b': 'b' * 128})
        with tempfile.TemporaryDirectory() as tmp:
            left, right = Path(tmp)/'l', Path(tmp)/'r'
            left.write_text(text); right.write_text(text)
            self.assertEqual(r.compare(left, right), [])
            right.write_text(text.replace('b' * 64, 'c' * 64) + f'{"d" * 64}  bin/d\n')
            found = r.compare(left, right)
            self.assertIn('differs: bin/b', found)
            self.assertTrue(any('only in' in f and 'bin/d' in f for f in found))
        for bad in ('zz  bin/a', f'{"a" * 64} bin/a', f'{"a" * 64}  bin/a\n{"b" * 64}  bin/a'):
            with self.assertRaises(ValueError): r.parse_sums(bad)

    def test_digests(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)/'x'; path.write_bytes(b'abc')
            d = r.digests(path)
            self.assertEqual(d['bytes'], 3)
            self.assertEqual(d['sha256'], 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')


class SbomTests(unittest.TestCase):
    def test_the_sbom_lists_every_locked_crate_and_module(self):
        data, _ = r.load_set(HERE/'RELEASE_SET.json')
        sbom = r.sbom(MAINNET, data)
        self.assertEqual(sbom['schema'], r.SBOM_SCHEMA)
        self.assertEqual(set(sbom['cargo']), {'node', 'node/consensus/pqc-http-adapter', 'sdk'})
        self.assertEqual(set(sbom['go']), {'node/consensus/cometbft', 'node/consensus/root-authorization'})
        rocks = [p for p in sbom['cargo']['node'] if p['name'] == 'librocksdb-sys']
        self.assertTrue(rocks and rocks[0]['checksum'])
        for module in sbom['go']['node/consensus/cometbft']:
            self.assertFalse(module['version'].endswith('/go.mod'))
            self.assertTrue(module['h1'].startswith('h1:'))
        # Deterministic: the same tree gives the same document.
        self.assertEqual(json.dumps(sbom, sort_keys=True), json.dumps(r.sbom(MAINNET, data), sort_keys=True))


if __name__ == '__main__': unittest.main(verbosity=2)
