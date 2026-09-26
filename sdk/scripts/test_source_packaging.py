#!/usr/bin/env python3
"""Small filesystem tests for protocol provenance and source packaging."""
import hashlib
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
import sys
sys.dont_write_bytecode = True
from check_protocol_vendor import verify
from package_sdk_source import package, source_files


class SourcePackagingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.shared = tempfile.TemporaryDirectory(prefix="dytallix-source-tests-")
        cls.snapshot = Path(cls.shared.name) / "snapshot"
        cls.snapshot.mkdir()
        root = Path(__file__).resolve().parents[1]
        for source in source_files(root):
            destination = cls.snapshot / source.relative_to(root)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)

    @classmethod
    def tearDownClass(cls):
        cls.shared.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="dytallix-package-case-")
        self.root = Path(self.temp.name) / "sdk"
        shutil.copytree(self.snapshot, self.root)
        self.vendor = self.root / "vendor/dytallix-protocol-types"
        self.manifest = self.root / "vendor/protocol-types-source.json"

    def tearDown(self):
        self.temp.cleanup()

    def test_standalone_check_needs_neither_git_nor_node(self):
        self.assertFalse((self.root / ".git").exists())
        self.assertFalse((self.root.parent / "dytallix-node").exists())
        self.assertEqual(verify(self.root)["status"], "PASS")

    def test_changed_missing_and_extra_source_files_reject(self):
        source = self.vendor / "src/ordinary.rs"
        original = source.read_bytes()
        source.write_bytes(original + b"\n")
        with self.assertRaises(ValueError):
            verify(self.root)
        source.unlink()
        with self.assertRaises(ValueError):
            verify(self.root)
        source.write_bytes(original)
        (self.vendor / "unrecorded.rs").write_text("unrecorded")
        with self.assertRaises(ValueError):
            verify(self.root)

    def test_symlinked_source_rejects(self):
        source = self.vendor / "src/ordinary.rs"
        outside = self.root.parent / "ordinary.rs"
        source.rename(outside)
        source.symlink_to(outside)
        with self.assertRaises(ValueError):
            verify(self.root)

    def test_duplicate_manifest_fields_and_path_escape_reject(self):
        original = self.manifest.read_text()
        self.manifest.write_text(original.replace('"schema_version": 1,', '"schema_version": 1, "schema_version": 1,', 1))
        with self.assertRaises(ValueError):
            verify(self.root)
        manifest = json.loads(original)
        manifest["files"][0]["path"] = "../outside"
        self.manifest.write_text(json.dumps(manifest))
        with self.assertRaises(ValueError):
            verify(self.root)

    def test_optional_canonical_source_comparison_detects_drift(self):
        node = self.root.parent / "canonical-node"
        manifest = json.loads(self.manifest.read_text())
        for record in manifest["files"]:
            path = node / record["source_path"]
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.vendor / record["path"], path)
        self.assertTrue(verify(self.root, node)["canonical_node_compared"])
        extra = node / "crates/protocol-types/src/new.rs"
        extra.write_text("unrecorded")
        with self.assertRaises(ValueError):
            verify(self.root, node)
        extra.unlink()
        (node / "crates/protocol-types/src/ordinary.rs").write_text("changed")
        with self.assertRaises(ValueError):
            verify(self.root, node)

    def test_archive_is_reproducible_and_extraction_is_complete(self):
        first = self.root.parent / "first.tar.gz"
        second = self.root.parent / "second.tar.gz"
        a = package(self.root, first)
        b = package(self.root, second)
        self.assertEqual(a["sha256"], b["sha256"])
        extracted = self.root.parent / "extracted"
        with tarfile.open(first) as archive:
            for member in archive.getmembers():
                self.assertTrue(member.isfile())
                self.assertTrue(member.name.startswith("dytallix-sdk/"))
                self.assertNotIn("..", Path(member.name).parts)
                target = extracted / member.name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.extractfile(member).read())
                target.chmod(member.mode)
        root = extracted / "dytallix-sdk"
        self.assertEqual(verify(root)["status"], "PASS")
        manifest = json.loads((root / "SOURCE-MANIFEST.json").read_text())
        self.assertEqual({r["path"] for r in manifest["files"]}, {p.relative_to(root).as_posix() for p in source_files(root)})
        for record in manifest["files"]:
            data = (root / record["path"]).read_bytes()
            self.assertEqual(len(data), record["bytes"])
            self.assertEqual(hashlib.sha256(data).hexdigest(), record["sha256"])
        self.assertNotIn("../../../dytallix-node", (root / "crates/dytallix-sdk/Cargo.toml").read_text())

    def test_new_browser_crate_source_is_included_without_build_output(self):
        crate = self.root / "crates/ordinary-browser"
        (crate / "src").mkdir(parents=True, exist_ok=True)
        (crate / "tests").mkdir(exist_ok=True)
        (crate / "Cargo.toml").write_text('[package]\nname = "ordinary-browser"\nversion = "0.1.0"\n')
        (crate / "src/lib.rs").write_text("// public codec source fixture\n")
        (crate / "tests/codec.rs").write_text("// public codec test fixture\n")
        (crate / "target").mkdir(exist_ok=True)
        (crate / "target/generated.wasm").write_bytes(b"excluded build artifact")
        output = self.root.parent / "browser-source.tar.gz"
        package(self.root, output)
        with tarfile.open(output) as archive:
            names = set(archive.getnames())
        for name in ("Cargo.toml", "src/lib.rs", "tests/codec.rs"):
            self.assertIn("dytallix-sdk/crates/ordinary-browser/" + name, names)
        self.assertNotIn("dytallix-sdk/crates/ordinary-browser/target/generated.wasm", names)

    def test_existing_output_and_modified_vendor_do_not_package(self):
        output = self.root.parent / "existing.tar.gz"
        output.write_bytes(b"preserve existing artifact")
        with self.assertRaises(ValueError):
            package(self.root, output)
        self.assertEqual(output.read_bytes(), b"preserve existing artifact")
        (self.vendor / "src/ordinary.rs").write_text("changed")
        with self.assertRaises(ValueError):
            package(self.root, self.root.parent / "invalid.tar.gz")


if __name__ == "__main__":
    unittest.main()
