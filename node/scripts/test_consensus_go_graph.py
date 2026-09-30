"""The Go graph screen fails on classical packages, remote-signer sources and Dytallix build tags."""
from pathlib import Path
import tempfile
import unittest

from check_consensus_go_graph import evaluate_graph, load_rules, remote_signer_files, tagged_sources

RULES = load_rules(Path(__file__).resolve().parents[1] / "consensus/cometbft")[1]


def graph(*paths, **flags):
    return [{"ImportPath": path, **flags} for path in paths]


class GoGraphTests(unittest.TestCase):
    def test_pqc_graph_passes(self):
        incomplete, prohibited, providers = evaluate_graph(RULES, graph(
            "crypto/mlkem", "crypto/sha3", "github.com/cometbft/cometbft/crypto/mldsa65",
            "golang.org/x/crypto/chacha20poly1305"))
        self.assertEqual((incomplete, prohibited, providers), ([], {}, {}))

    def test_classical_packages_fail(self):
        for path in ["crypto/ed25519", "crypto/ecdsa", "crypto/rsa", "crypto/tls", "crypto/x509",
                     "golang.org/x/crypto/curve25519", "github.com/cometbft/cometbft/crypto/secp256k1",
                     "github.com/cometbft/cometbft/lp2p", "github.com/libp2p/go-libp2p"]:
            self.assertIn(path, evaluate_graph(RULES, graph(path))[1])

    def test_incomplete_graph_fails(self):
        self.assertEqual(evaluate_graph(RULES, graph("example.com/broken", Incomplete=True))[0],
                         ["example.com/broken"])

    def test_boring_markers_are_reported_for_review_not_failed(self):
        _, prohibited, providers = evaluate_graph(RULES, graph("crypto/internal/boring"))
        self.assertEqual(prohibited, {})
        self.assertIn("crypto/internal/boring", providers)

    def test_remote_signer_sources_are_detected(self):
        self.assertEqual(remote_signer_files(["doc.go", "errors.go", "file.go"]), [])
        self.assertEqual(remote_signer_files(["file.go", "signer_client.go", "socket_dialers.go"]),
                         ["signer_client.go", "socket_dialers.go"])

    def test_dytallix_build_constraints_are_detected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "a").mkdir()
            (root / "a/plain.go").write_text("package a\n")
            (root / "a/other_tag.go").write_text("//go:build linux\n\npackage a\n")
            self.assertEqual(tagged_sources(root), [])
            (root / "a/legacy.go").write_text("//go:build !dytallix_pqc_only\n\npackage a\n")
            self.assertEqual(tagged_sources(root), ["a/legacy.go"])


if __name__ == "__main__":
    unittest.main()
