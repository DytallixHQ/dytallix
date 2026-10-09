"""The staging chain's records and plan (host setup H4). The whole pipeline,
with the release's real tools and a real install, is the Host install
workflow."""
import json
import unittest

import staging_chain as sc
import staging_host as sh


class StagingChainTests(unittest.TestCase):
    def setUp(self):
        self.rehearsal = sc.read_json(sc.REHEARSAL / 'records.json')
        self.plan = sh.staging_plan('10.1.0.7')
        for n, host in enumerate(self.plan['hosts']):
            host['validator_public_key_base64'] = f'key-{n}'

    def test_plan_puts_only_the_validator_on_this_host(self):
        self.assertEqual(self.plan['chain_id'], 'dytallix-staging-1')
        addresses = {h['label']: h['p2p'].rsplit(':', 1)[0] for h in self.plan['hosts']}
        self.assertEqual(addresses['validator-1'], '10.1.0.7')
        self.assertEqual(len(set(addresses.values())), len(addresses))
        endpoint = next(h for h in self.plan['hosts'] if h['role'] == 'endpoint')
        self.assertTrue(endpoint['channel'].startswith(addresses['endpoint-1'] + ':'))
        self.assertTrue(all(a == '10.1.0.7' or a.startswith('192.0.2.') for a in addresses.values()))

    def test_records_carry_the_plans_validators(self):
        records = sc.staging_records(self.plan, self.rehearsal, '2026-10-06T12:00:00Z')
        self.assertEqual(records['validators'], [dict(self.rehearsal['validators'][0], validator_id='validator-1',
                                                      consensus_public_key_base64='key-0')])
        self.assertEqual((records['chain_id'], records['genesis_time']), ('dytallix-staging-1', '2026-10-06T12:00:00Z'))
        self.assertTrue(all(d['validator_id'] == 'validator-1' for d in records['delegations']))
        # Accounts, and so the DGT total, are the rehearsal's.
        self.assertEqual(records['accounts'], self.rehearsal['accounts'])
        self.assertIn('Not for any network', records['boundary'])
        self.assertNotEqual(self.rehearsal['validators'][0]['consensus_public_key_base64'], 'key-0')

    def test_refuses_a_network_identity_or_no_validator(self):
        identity = sc.read_json(sc.IDENTITY)
        for chain in (identity['chain_id'], 'dytallix-mainnet-2', 'dytallix-production-1'):
            plan = json.loads(json.dumps(self.plan))
            plan['chain_id'] = chain
            with self.assertRaises(sc.Invalid, msg=chain):
                sc.staging_records(plan, self.rehearsal, '2026-10-06T12:00:00Z')
        plan = json.loads(json.dumps(self.plan))
        plan['hosts'] = [h for h in plan['hosts'] if h['role'] != 'validator']
        with self.assertRaises(sc.Invalid):
            sc.staging_records(plan, self.rehearsal, '2026-10-06T12:00:00Z')

    def test_control_kits_replace_only_the_root_control_keys(self):
        records = sc.staging_records(self.plan, self.rehearsal, '2026-10-06T12:00:00Z')
        authorities = {purpose: {'keys': [{'key_id': purpose, 'public_key_hex': '00'}], 'threshold': 3}
                       for purpose in ('freeze', 'resume', 'upgrade')}
        changed = sc.with_control_kits(records, authorities)
        root = changed['root']
        self.assertEqual((root['emergency']['freeze'], root['emergency']['resume'], root['upgrade']['authority']),
                         (authorities['freeze'], authorities['resume'], authorities['upgrade']))
        # The rest of the records, and the input, are unchanged.
        self.assertEqual(records['root'], self.rehearsal['root'])
        self.assertEqual({k: v for k, v in changed.items() if k != 'root'},
                         {k: v for k, v in records.items() if k != 'root'})
        for section in ('emergency', 'upgrade'):
            kept = {k: v for k, v in root[section].items() if k not in ('freeze', 'resume', 'authority')}
            self.assertEqual(kept, {k: v for k, v in records['root'][section].items()
                                    if k not in ('freeze', 'resume', 'authority')})

    def test_only_the_notice_is_a_staging_value(self):
        approved = sc.read_json(sc.E05_VALUES)['values']
        staging = sc.genesis_values()['values']
        changed = [(a, b) for a, b in zip(approved, staging) if a != b]
        self.assertEqual(len(approved), len(staging))
        self.assertEqual(sorted(b['path'] for _, b in changed), sorted(sc.NOTICE_PATHS))
        for before, after in changed:
            self.assertEqual((before['approved'], after['approved']), (120960, sc.STAGING_NOTICE_BLOCKS))
            self.assertEqual(after['approval_record'], sc.DRILL_APPROVAL)
            self.assertTrue((sc.MAINNET / sc.DRILL_APPROVAL).is_file())


if __name__ == '__main__':
    unittest.main()
