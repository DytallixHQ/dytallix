import json
from pathlib import Path
import shutil
import tempfile
import unittest

from check_module_boundaries import check

ROOT = Path(__file__).resolve().parents[1]


class ModuleBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        policy = ROOT / 'docs/architecture/module-policy.json'
        (self.root / 'docs/architecture').mkdir(parents=True)
        shutil.copyfile(policy, self.root / 'docs/architecture/module-policy.json')
        for rule in json.loads(policy.read_text())['modules'].values():
            target = self.root / rule['path']
            target.mkdir(parents=True)
            shutil.copyfile(ROOT / rule['path'] / 'Cargo.toml', target / 'Cargo.toml')

    def tearDown(self):
        self.directory.cleanup()

    def test_declared_graph_passes(self):
        self.assertEqual(check(self.root), [])

    def test_network_dependency_in_protocol_module_fails(self):
        path = self.root / 'crates/protocol-types/Cargo.toml'
        path.write_text(path.read_text() + '\nreqwest = "0.11"\n')
        self.assertTrue(any('forbidden dependencies edge to reqwest' in error for error in check(self.root)))

    def test_alias_does_not_hide_node_dependency(self):
        path = self.root / 'crates/protocol-types/Cargo.toml'
        path.write_text(path.read_text() + '\nhelper = { package = "dytallix-fast-node", path = "../../dytallix-fast-launch/node" }\n')
        self.assertTrue(any('dytallix-fast-node' in error for error in check(self.root)))

    def test_unregistered_module_fails(self):
        path = self.root / 'crates/unregistered'
        path.mkdir()
        (path / 'Cargo.toml').write_text('[package]\nname="unregistered"\n')
        self.assertTrue(any('no dependency policy' in error for error in check(self.root)))


if __name__ == '__main__':
    unittest.main()
