"""Release graph requirements reject dependency and feature drift."""
import unittest
from check_consensus_cargo_profile import REQUIRED, inspect_tree


def tree(features="default-rng,ml-dsa-65", extra=""):
    return "\n".join(f"{name} v0.1.0|{features if name == 'fips204' else 'default'}" for name in sorted(REQUIRED)) + "\n" + extra


class ConsensusGraphTests(unittest.TestCase):
    def test_selected_dependencies_and_mldsa65_pass(self):
        _, prohibited, missing, features, unexpected, absent = inspect_tree(tree())
        self.assertEqual((prohibited, missing, unexpected, absent), ([], [], [], []))
        self.assertEqual(features, ["default-rng", "ml-dsa-65"])

    def test_legacy_pqc_implementations_fail(self):
        for package in ["dytallix-pqc", "pqcrypto-dilithium", "pqcrypto-falcon", "pqcrypto-kyber", "pqcrypto-sphincsplus"]:
            self.assertIn(package, inspect_tree(tree(extra=f"{package} v0.1.0|default"))[1])

    def test_classical_and_network_packages_fail(self):
        for package in ["rsa", "rustls", "ring", "openssl-sys", "reqwest"]:
            self.assertIn(package, inspect_tree(tree(extra=f"{package} v0.1.0|default"))[1])

    def test_unified_development_fips_features_fail(self):
        for feature in ["ml-dsa-44", "ml-dsa-87", "default"]:
            self.assertEqual(inspect_tree(tree(features=f"default-rng,ml-dsa-65,{feature}"))[4], [feature])

    def test_missing_operational_fips_feature_fails(self):
        self.assertEqual(inspect_tree(tree(features="default-rng"))[5], ["ml-dsa-65"])
        self.assertEqual(inspect_tree(tree(features=""))[5], ["default-rng", "ml-dsa-65"])


if __name__ == "__main__":
    unittest.main()
