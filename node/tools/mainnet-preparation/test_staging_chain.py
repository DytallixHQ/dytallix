"""The staging chain's records and plan (host setup H4). The whole pipeline,
with the release's real tools and a real install, is the Host install
workflow."""
import json
import unittest

import resolve_genesis_inputs
import resolve_host_values
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
        # Two seats (P01, 11 October 2026): the sentry serves the client channel.
        self.assertEqual(sorted(h['role'] for h in self.plan['hosts']), ['sentry', 'validator'])
        public = next(h for h in self.plan['hosts'] if h['channel'])
        self.assertEqual(public['label'], 'sentry-1')
        self.assertTrue(public['channel'].startswith(addresses['sentry-1'] + ':'))
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

    def changed(self, profile, snapshot_interval=None):
        approved = sc.read_json(sc.E05_VALUES)['values']
        values, record = sc.timing_values(profile, snapshot_interval)
        self.assertEqual(len(approved), len(values['values']))
        changed = {b['path']: (a, b) for a, b in zip(approved, values['values']) if a != b}
        self.assertEqual(sorted(changed), sorted(c['path'] for c in record['changed']))
        for c in record['changed']:
            before, after = changed[c['path']]
            self.assertEqual((before['approved'], after['approved']), (c['production'], c['value']))
            self.assertEqual(type(before['approved']), type(after['approved']))
            self.assertEqual(after['approval_record'], record['approval_record'])
            self.assertTrue((sc.MAINNET / record['approval_record']).is_file())
        return changed, record

    def test_the_ci_profile_replaces_only_the_notice_and_the_snapshot_interval(self):
        changed, _ = self.changed('ci', 10)
        self.assertEqual(sorted(changed), sorted(['upgrade.min_notice_blocks', 'release_handover.v2.min_notice_blocks',
                                                  sc.SNAPSHOT_PATH]))
        self.assertEqual(changed['upgrade.min_notice_blocks'][1]['approved'], 20)
        self.assertEqual(sorted(self.changed('ci')[0]), ['release_handover.v2.min_notice_blocks', 'upgrade.min_notice_blocks'])

    def test_the_production_profile_replaces_nothing(self):
        values, record = sc.timing_values('production')
        self.assertEqual(values, sc.read_json(sc.E05_VALUES))
        self.assertEqual((record['changed'], record['approval_record']), ([], None))
        with self.assertRaises(sc.Invalid):
            sc.timing_values('production', 10)

    def test_the_rehearsal_profile_fits_each_lifecycle_in_a_run(self):
        # P01, 12 October 2026: R01 to R06's shortened timings.
        changed, record = self.changed('rehearsal')
        self.assertEqual(len(changed), 11)
        self.assertEqual(changed['lifecycle.evidence_max_age_blocks'][1]['approved'], '2880')
        self.assertEqual(changed['config.toml [statesync] trust_period'][1]['approved'], '1h')
        derived = record['derived']
        self.assertEqual((derived['unbond_maturity_seconds'], derived['governance_cycle_seconds']), (15000, 9000))
        self.assertLess(derived['newest_copy_max_age_seconds'], derived['restore_trust_seconds'])
        # In the worst case a rehearsal restore must start within about 88 minutes of the loss.
        self.assertEqual(derived['restore_deadline_after_loss_seconds'], 13372 - 8100)
        self.assertLess(derived['state_sync_trust_seconds'], 14400)

    def test_the_rehearsal_values_resolve_for_the_builder_and_the_hosts(self):
        values, _ = sc.timing_values('rehearsal')
        records = sc.read_json(sc.REHEARSAL / 'records.json')
        inputs, _ = resolve_genesis_inputs.resolve(values, sc.read_json(sc.GENESIS_PROPOSALS), records, mode='production')
        text = json.dumps(inputs)
        for needle in ('"evidence_max_age_blocks": 2880', '"evidence_max_age_seconds": 14400',
                       '"processing_margin_blocks": 120', '"voting_period_blocks": 720', '"timelock_blocks": 360'):
            self.assertIn(needle, text)
        host, _ = resolve_host_values.resolve(values, sc.read_json(sc.HOST_PROPOSALS))
        self.assertIn('"trust_period": "1h"', json.dumps(host))

    def test_the_timings_match_the_tools_they_describe(self):
        timings = sc.read_json(sc.TIMINGS)
        # The backup timer: hourly, up to 15 minutes late.
        self.assertIn(b'OnCalendar=hourly', sc.host_files.BACKUP_TIMER)
        self.assertIn(b'RandomizedDelaySec=15m', sc.host_files.BACKUP_TIMER)
        self.assertEqual(timings['backup_interval_seconds'], 3600 + 900)
        # The engine's restore margin.
        restore = (sc.MAINNET / 'node' / 'consensus' / 'cometbft' / 'cmd' / 'dytallix-pqc-engine' / 'restore.go').read_text()
        self.assertIn(f'restoreTrustDivisor = {timings["restore_trust_divisor"]}\n', restore)

    def test_a_profile_that_breaks_a_rule_is_refused(self):
        def refused(change, message):
            timings = sc.read_json(sc.TIMINGS)
            change(timings['profiles']['rehearsal']['values'])
            with self.assertRaises(sc.Invalid) as caught:
                sc.timing_values('rehearsal', timings=timings)
            self.assertIn(message, str(caught.exception))
        refused(lambda v: v.update({'config.toml [statesync] trust_period': '4h'}), 'trust period')
        refused(lambda v: v.update({'service.snapshots.interval_blocks': 2000}), 'newest copy')
        refused(lambda v: v.update({'lifecycle.evidence_max_age_seconds': 14000}), 'evidence age is 2880 blocks')
        refused(lambda v: v.update({'governance.ballot.voting_period_blocks': 17000}), 'governance_cycle_seconds')
        refused(lambda v: v.update({'monitor.nonexistent': 1}), 'not an approved E05 value')
        with self.assertRaises(sc.Invalid):
            sc.timing_values('fast')

    def keys(self, validators=1):
        return {label: f'key-{label}' for label in sc.rehearsal_labels(validators)}

    def test_spendable_records_hold_the_approved_allocation(self):
        records = sc.spendable_records(self.plan, self.rehearsal, '2026-10-06T12:00:00Z', self.keys())
        accounts = {a['label']: a for a in records['accounts']}
        self.assertEqual([a['label'] for a in records['accounts']], sc.rehearsal_labels(1))
        dgt = {label: int(a['udgt']) // sc.DGT for label, a in accounts.items()}
        drt = {label: int(a['udrt']) // sc.DRT for label, a in accounts.items()}
        self.assertEqual(sum(dgt.values()), 1_000_000_000)
        self.assertEqual(sum(drt.values()), 1_000_000)
        # The validator's self-bond comes out of Ecosystem growth, the holders'
        # grants out of Team and advisors (genesis framework, section 1).
        self.assertEqual((dgt['ecosystem-growth'], dgt['operator-1'], dgt['team-and-advisors']),
                         (299_900_000, 100_000, 199_900_000))
        self.assertEqual([dgt[f'kit-holder-{n}'] for n in range(1, 6)], [20_000] * 5)
        self.assertEqual((dgt['public-sale'], dgt['private-sale'], dgt['reserve'], dgt['treasury']),
                         (150_000_000, 150_000_000, 200_000_000, 0))
        self.assertEqual(drt['ecosystem-growth'], 1_000_000 - 1_000 - 5 * 2_000 - 1_000)
        self.assertTrue(all(a['vesting'] == {'kind': 'unlocked'} for a in records['accounts']))
        self.assertTrue(all(a['origin_public_key_base64'] == f'key-{a["label"]}' for a in records['accounts']))
        self.assertEqual(records['delegations'], [])
        self.assertEqual([(v['validator_id'], v['operator'], v['self_bond_udgt']) for v in records['validators']],
                         [('validator-1', 'operator-1', str(100_000 * sc.DGT))])

    def test_each_validator_has_an_operator_carved_from_ecosystem_growth(self):
        plan = json.loads(json.dumps(self.plan))
        extra = dict(next(h for h in plan['hosts'] if h['role'] == 'validator'), label='validator-2')
        plan['hosts'].append(extra)
        records = sc.spendable_records(plan, self.rehearsal, '2026-10-06T12:00:00Z', self.keys(2))
        accounts = {a['label']: int(a['udgt']) // sc.DGT for a in records['accounts']}
        self.assertEqual((accounts['ecosystem-growth'], accounts['operator-1'], accounts['operator-2']),
                         (299_800_000, 100_000, 100_000))
        self.assertEqual(sum(accounts.values()), 1_000_000_000)

    def test_public_keys_come_from_the_keystore(self):
        import base64
        import tempfile
        from pathlib import Path
        raw = bytes(range(256)) * 7 + bytes(160)
        self.assertEqual(len(raw), 1952)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'keystore.json'
            path.write_text(json.dumps({'version': 2, 'entries': [{'name': 'reserve', 'public_key': list(raw)}]}))
            self.assertEqual(sc.keystore_public_keys(path, ['reserve']), {'reserve': base64.b64encode(raw).decode()})
            with self.assertRaises(sc.Invalid):
                sc.keystore_public_keys(path, ['reserve', 'treasury'])
            path.write_text(json.dumps({'entries': [{'name': 'reserve', 'public_key': list(raw[:64])}]}))
            with self.assertRaises(sc.Invalid):
                sc.keystore_public_keys(path, ['reserve'])

    def test_the_mapping_names_the_row_each_account_stands_for(self):
        records = sc.spendable_records(self.plan, self.rehearsal, '2026-10-06T12:00:00Z', self.keys())
        native = {'accounts': [{'address': f'tdytallix1{a["label"]}'} for a in records['accounts']]}
        result = sc.mapping(records, native, 'dytallix-staging-1')
        rows = {a['label']: a for a in result['accounts']}
        self.assertEqual(rows['kit-holder-3']['stands_for'], "Root key holder 3's grant")
        self.assertIn('self-bond', rows['operator-1']['stands_for'])
        self.assertEqual(rows['treasury']['address'], 'tdytallix1treasury')
        self.assertIn('Not for any network', result['boundary'])
        with self.assertRaises(sc.Invalid):
            sc.mapping(records, {'accounts': native['accounts'][:-1]}, 'dytallix-staging-1')


if __name__ == '__main__':
    unittest.main()
