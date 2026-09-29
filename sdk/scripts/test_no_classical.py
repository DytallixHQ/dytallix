#!/usr/bin/env python3
"""Tests for the lockfile check (E04 gap 19)."""
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
from check_no_classical import PROHIBITED, check, packages


def lock(*names):
    return 'version = 4\n' + ''.join(
        f'\n[[package]]\nname = "{name}"\nversion = "1.0.0"\ndependencies = [\n "sha2",\n]\n'
        for name in names)


class NoClassicalTests(unittest.TestCase):
    def test_classical_and_tls_crates_are_prohibited(self):
        for name in ["ring", "rustls", "rustls-webpki", "tokio-rustls", "hyper-rustls", "native-tls",
                     "openssl", "openssl-sys", "openssl-probe", "security-framework", "schannel",
                     "hyper-tls", "webpki-roots", "x509-parser", "ed25519-dalek", "ed25519-zebra",
                     "curve25519-dalek", "x25519-dalek", "k256", "p256", "rsa", "ecdsa",
                     "elliptic-curve", "secp256k1", "libsecp256k1", "blst", "aws-lc-rs", "quinn"]:
            self.assertTrue(PROHIBITED.fullmatch(name), name)

    def test_post_quantum_and_symmetric_crates_are_allowed(self):
        for name in ["fips203", "fips204", "pqcrypto-sphincsplus", "aes-gcm", "argon2", "hkdf",
                     "sha2", "sha3", "blake3", "hyper", "hyper-util", "tokio", "subtle", "zeroize",
                     "ringbuf", "rsa-oaep-fake-free", "p256-free"]:
            self.assertFalse(PROHIBITED.fullmatch(name), name)

    def test_names_come_from_package_entries_only(self):
        self.assertEqual(packages(lock("sha2", "hyper")), ["hyper", "sha2"])
        # A dependency list entry is not a package.
        self.assertNotIn("ring", packages(lock("sha2") + ' "ring",\n'))

    def test_a_lockfile_with_a_prohibited_package_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            clean = Path(directory) / "clean.lock"
            dirty = Path(directory) / "dirty.lock"
            clean.write_text(lock("fips204", "sha3"))
            dirty.write_text(lock("fips204", "rustls", "ring"))
            self.assertEqual(check([clean])["status"], "PASS")
            report = check([clean, dirty])
            self.assertEqual(report["status"], "FAIL")
            self.assertEqual(report["lockfiles"][str(dirty)]["prohibited"], ["ring", "rustls"])
            empty = Path(directory) / "empty.lock"
            empty.write_text("version = 4\n")
            with self.assertRaises(ValueError):
                check([empty])

    def test_this_sdk_lockfile_passes(self):
        root = Path(__file__).resolve().parents[1]
        self.assertEqual(check([root / "Cargo.lock"])["status"], "PASS")


if __name__ == "__main__":
    unittest.main()
