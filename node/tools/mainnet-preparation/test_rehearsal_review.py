"""The binding review of the full rehearsal genesis (E05-d2). Synthetic records only; nothing is accepted."""
import copy
import hashlib
import json
from pathlib import Path
import unittest
import check_bindings as c
import config_checks
import genesis_rehearsal_records as rehearsal

FIXTURE = Path(__file__).resolve().parent/'fixtures'/'genesis-rehearsal'
EXPECTED_MISSING = {'consensus_configuration', 'network_configuration', 'root_authorization', 'approval_bundle', 'service_configuration'}


def raw(name): return (FIXTURE/name).read_bytes()


class RehearsalReviewTests(unittest.TestCase):
    def setUp(self):
        self.bindings = json.loads(raw('review-bindings.json'))
        self.records_raw = raw('review-records.json')
        self.native_raw, self.config_raw, self.engine_raw = raw('native-genesis.json'), raw('application-config.json'), raw('genesis.json')
        self.config = json.loads(self.config_raw)

    def review(self, config=None, engine_raw=None, manifest=True):
        """Review the rehearsal, optionally with a changed configuration or engine genesis."""
        config_raw = self.config_raw if config is None else json.dumps(config, separators=(',', ':')).encode()
        engine_raw = self.engine_raw if engine_raw is None else engine_raw
        bindings = copy.deepcopy(self.bindings)
        bindings['source_digests'].update(application_config_sha256=c.digest(config_raw), engine_genesis_sha256=c.digest(engine_raw))
        return c.validate(bindings, json.loads(self.records_raw), self.records_raw, self.native_raw, config_raw, None, engine_raw,
                          raw('BUILD_MANIFEST.json') if manifest else None)

    def refused(self, code, config=None, engine_raw=None):
        errors = self.review(config, engine_raw, manifest=False)['errors']
        self.assertTrue(any(code in e['code'] for e in errors), errors)

    def test_the_review_packet_is_current(self):
        records_text, bindings_text = rehearsal.review_packet(FIXTURE)
        self.assertEqual(records_text.encode(), self.records_raw, 'rerun genesis_rehearsal_records.py --out fixtures/genesis-rehearsal')
        self.assertEqual(bindings_text.encode(), raw('review-bindings.json'))

    def test_the_rehearsal_passes_every_supported_review(self):
        r = self.review()
        self.assertEqual(r['errors'], [])
        self.assertEqual(r['status'], 'BLOCKED')
        self.assertFalse(r['production_accepted'] or r['runtime_complete'])
        for check in ('full_application_configuration', 'engine_genesis_binding', 'build_manifest_binding', 'beneficiary_amount_vesting_stake_bindings', 'operator_key_bindings', 'chain_identity_record_binding', 'governance_parameters_exact_source_binding', 'genesis_time_exact_source_binding'):
            self.assertIn(check, r['checks'])
        self.assertEqual(r['reviewed_sections'], list(config_checks.SECTIONS) + ['root_controls'])
        self.assertEqual(set(r['missing']), EXPECTED_MISSING)
        self.assertTrue(any(u['field'] == 'production_activation' for u in r['unsupported']))

    def test_addresses_derive_from_origin_keys(self):
        account = next(iter(self.config['recovery']['accounts'].values()))
        account['recovery']['active_key']['public_key'][0] ^= 1
        self.refused('account_address_not_derived_from_origin_key', self.config)

    def test_validator_power_is_bonded_stake(self):
        self.config['validators'][1]['power'] += 1
        self.refused('validator_power_differs_from_bonded_stake', self.config)

    def test_fee_cap_covers_the_largest_signable_fee(self):
        fees = self.config['ordinary']['fee_profile']
        fees['max_fee_cap'] = str(int(fees['max_fee_cap']) - 1)
        self.config['governance']['fee_profile']['base'] = copy.deepcopy(fees)
        self.refused('ordinary_fee_cap_below_largest_fee', self.config)

    def test_governance_base_is_the_ordinary_profile(self):
        self.config['governance']['fee_profile']['base']['gas_price'] = '11'
        self.refused('governance_base_differs_from_ordinary_profile', self.config)

    def test_costs_lie_within_governance_bounds(self):
        self.config['governance']['parameter_bounds']['resource_cost']['max'] = 10000
        self.refused('fee_values_outside_governance_bounds', self.config)

    def test_the_reference_send_lies_within_its_bound(self):
        # The rehearsal's approved profile prices the reference basic Send at
        # 1 DRT; the node's own check refuses the same profiles.
        base = self.config['governance']['fee_profile']['base']
        self.assertEqual(config_checks.reference_send_fee(base), 1_000_000)
        bound = self.config['governance']['parameter_bounds']['reference_send_fee_udrt']
        self.assertEqual(bound, {'min': 100_000, 'max': 10_000_000})
        bound['max'] = 999_999
        self.refused('reference_send_fee_outside_governance_bounds', self.config)
        bound['max'] = 10_000_000
        bound['min'] = 1_000_001
        self.refused('reference_send_fee_outside_governance_bounds', self.config)
        del self.config['governance']['parameter_bounds']['reference_send_fee_udrt']
        self.refused('Missing or unknown contract fields', self.config)
        # Above the floor, every per-byte cost counts: 5,565 wire bytes at 2.
        self.assertEqual(config_checks.reference_send_fee(dict(base, minimum_gas='1')), 476_160)

    def test_validator_proof_profile_digest_is_recomputed(self):
        self.config['ordinary']['fee_profile']['validator_proof_profile_digest'][0] ^= 1
        self.config['governance']['fee_profile']['base'] = copy.deepcopy(self.config['ordinary']['fee_profile'])
        self.refused('validator_proof_profile_digest_mismatch', self.config)

    def test_recovery_accounts_bind_the_native_genesis(self):
        account = next(iter(self.config['recovery']['accounts'].values()))
        account['recovery']['domain']['genesis_digest'][0] ^= 1
        self.refused('recovery_genesis_digest_mismatch', self.config)

    def test_upgrade_keys_are_not_emergency_keys(self):
        self.config['upgrade']['authority']['keys'][0] = copy.deepcopy(self.config['emergency']['freeze_authority']['keys'][0])
        self.config['upgrade']['authority']['keys'].sort(key=lambda k: k['key_id'])
        self.refused('upgrade_key_holds_emergency_role', self.config)

    def test_root_policies_bind_the_native_genesis(self):
        self.config['emergency']['v2']['genesis_sha256'] = '0' * 64
        self.refused('emergency_v2_binding', self.config)

    def test_development_gates_are_reported(self):
        self.config['emergency']['development_only'] = False
        self.refused('emergency_development_gate', self.config)

    def test_engine_genesis_carries_the_exact_native_genesis(self):
        self.refused('engine_app_state_not_the_exact_native_genesis', engine_raw=self.engine_raw.replace(b'"udrt":"0"', b'"udrt":"1"', 1))

    def test_engine_evidence_equals_the_lifecycle(self):
        self.refused('engine_evidence_differs_from_lifecycle', engine_raw=self.engine_raw.replace(b'"max_age_num_blocks":"241920"', b'"max_age_num_blocks":"241921"'))

    def test_engine_validators_equal_the_application(self):
        self.config['validators'][0]['power'] -= 1
        self.refused('engine_validators_differ_from_application', self.config)

    def test_the_manifest_binds_the_files(self):
        manifest = json.loads(raw('BUILD_MANIFEST.json'))
        manifest['files']['genesis.json']['sha256'] = '0' * 64
        files = {'native-genesis.json': self.native_raw, 'application-config.json': self.config_raw, 'genesis.json': self.engine_raw}
        with self.assertRaisesRegex(ValueError, 'manifest_file_digest_mismatch'):
            config_checks.manifest(manifest, files)

    def test_bech32m_matches_a_known_vector(self):
        # BIP 350 test vector: the empty-data Bech32m string for the "a" prefix.
        self.assertEqual(config_checks.bech32m('a', b''), 'a1lqfn3a')


if __name__ == '__main__': unittest.main(verbosity=2)
