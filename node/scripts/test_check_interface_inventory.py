import copy
import tempfile
import unittest
from pathlib import Path

import check_interface_inventory as inventory


def entry(**changes):
    base = {
        "id": "abci-query-x",
        "group": "abci_query",
        "exposure": "external",
        "path": "/x",
        "variant": "X",
        "format": "JSON",
        "version": {"field": "version", "constant": {"file": "a.rs", "name": "X_VERSION", "value": 1}},
        "source": {"file": "a.rs", "symbol": "fn query_x("},
        "tests": [{"file": "a.rs", "name": "x_is_served"}],
    }
    base.update(changes)
    return base


class InventoryCheckTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)
        (self.root / "a.rs").write_text(
            "pub const X_VERSION: u16 = 1;\nfn query_x() {}\n#[test]\nfn x_is_served() {}\n"
        )
        (self.root / "b.go").write_text("const (\n\tFormatVersion = 1\n)\nfunc TestB(t *testing.T) {}\n")

    def tearDown(self):
        self.dir.cleanup()

    def errors(self, *entries):
        return inventory.check({"version": 1, "interfaces": list(entries)}, self.root)

    def test_a_consistent_entry_passes(self):
        go = entry(id="engine-b", group="operator_output", exposure="operator",
                   version={"field": "v", "constant": {"file": "b.go", "name": "FormatVersion", "value": 1}},
                   source={"file": "b.go", "symbol": "FormatVersion"},
                   tests=[{"file": "b.go", "name": "TestB"}])
        self.assertEqual(self.errors(entry(), go), [])

    def test_drift_from_the_source_is_refused(self):
        cases = [
            entry(version={"field": "version", "constant": {"file": "a.rs", "name": "X_VERSION", "value": 2}}),
            entry(version={"field": "version", "literal": {"file": "a.rs", "text": "absent-v1"}}),
            entry(version={"none": " "}),
            entry(version={"field": "version"}),
            entry(source={"file": "a.rs", "symbol": "fn query_y("}),
            entry(source={"file": "missing.rs", "symbol": "x"}),
            entry(tests=[{"file": "a.rs", "name": "y_is_served"}]),
            entry(group="rest"),
            entry(exposure="public"),
            entry(variant=None),
        ]
        for case in cases:
            with self.subTest(case=case):
                self.assertNotEqual(self.errors(case), [])

    def test_duplicate_ids_and_versions_are_refused(self):
        self.assertIn("duplicate id abci-query-x", self.errors(entry(), copy.deepcopy(entry())))
        self.assertTrue(inventory.check({"version": 2, "interfaces": []}, self.root))

    def test_the_committed_inventory_passes(self):
        committed = inventory.json.loads(inventory.INVENTORY.read_text(encoding="utf-8"))
        self.assertEqual(inventory.check(committed), [])


if __name__ == "__main__":
    unittest.main()
