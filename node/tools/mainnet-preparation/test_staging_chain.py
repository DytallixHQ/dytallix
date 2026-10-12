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

    def changed(self, values):
        approved = sc.read_json(sc.E05_VALUES)['values']
        self.assertEqual(len(approved), len(values['values']))
        return [(a, b) for a, b in zip(approved, values['values']) if a != b]

    def test_production_is_the_default_and_changes_nothing(self):
        values, record = sc.timing()
        self.assertEqual(values, sc.read_json(sc.E05_VALUES))
        self.assertEqual((record['profile'], record['values']), ('production', []))
        self.assertIn('R07', record['runs'])

    def test_ci_changes_only_the_notice_and_the_snapshot_interval(self):
        values, record = sc.timing('ci')
        changed = self.changed(values)
        self.assertEqual(sorted(b['path'] for _, b in changed), sorted(sc.NOTICE_PATHS))
        for before, after in changed:
            self.assertEqual((before['approved'], after['approved']), (120960, sc.STAGING_NOTICE_BLOCKS))
            self.assertEqual(after['approval_record'], sc.DRILL_APPROVAL)
            self.assertTrue((sc.MAINNET / sc.DRILL_APPROVAL).is_file())
        values, record = sc.timing('ci', snapshot_interval=10)
        self.assertEqual(sorted(r['path'] for r in record['values']),
                         sorted(sc.NOTICE_PATHS + ('service.snapshots.interval_blocks',)))
        self.assertEqual(len(self.changed(values)), 3)

    def test_a_snapshot_interval_or_an_unknown_profile_is_refused(self):
        for profile, interval in (('production', 10), ('rehearsal', 10), ('ci', 0), ('ci', 17281), ('fast', None)):
            with self.assertRaises(sc.Invalid, msg=(profile, interval)):
                sc.timing(profile, interval)

    def production(self):
        rows = sc.read_json(sc.E05_VALUES)['values']
        return {r['path']: int(r['approved']) if isinstance(r.get('approved'), str) and r['approved'].isdigit()
                else r.get('approved') for r in rows}

    def test_the_derivation_rules_give_the_approved_production_values(self):
        production = self.production()
        for path, value in sc.derived(production, 5).items():
            self.assertEqual(value, production[path], path)
        self.assertEqual(sc.violations(production, 5, 'production'), [])

    def test_the_restore_margin_is_the_engines(self):
        source = (sc.NODE / 'consensus' / 'cometbft' / 'cmd' / 'dytallix-pqc-engine' / 'restore.go').read_text()
        self.assertIn('restoreTrustMarginDivisor = 14', source)
        self.assertIn('evidence - evidence/restoreTrustMarginDivisor', source)
        self.assertEqual(sc.RESTORE_TRUST_MARGIN_DIVISOR, 14)
        # 312 hours (13 days) with the approved 14-day evidence age.
        self.assertEqual(sc.restore_trust_seconds(1_209_600), 312 * 3600)
        self.assertEqual(sc.timing()[1]['restore_trust_seconds'], 312 * 3600)

    def test_the_rehearsal_set_passes_and_a_two_hour_evidence_age_would_not(self):
        chosen = dict(self.production(), **sc.REHEARSAL_TIMINGS)
        chosen.update(sc.derived(chosen, 5))
        self.assertEqual(sc.violations(chosen, 5, 'rehearsal'), [])
        # The plan's first proposal, a 2-hour evidence age, is below a
        # backup's 3-hour freshness, so a fresh copy could not be restored.
        chosen = dict(self.production(), **dict(sc.REHEARSAL_TIMINGS, **{'lifecycle.evidence_max_age_blocks': 1440}))
        chosen.update(sc.derived(chosen, 5))
        found = sc.violations(chosen, 5, 'rehearsal')
        self.assertEqual(len(found), 1)
        self.assertIn('restore', found[0])

    def test_the_rehearsal_profile_records_every_value_beside_production(self):
        values, record = sc.timing('rehearsal')
        rows = {r['path']: r for r in record['values']}
        self.assertEqual(record['runs'], 'R01 to R06')
        self.assertEqual({p for p, r in rows.items() if r['source'] == 'plan'}, set(sc.REHEARSAL_TIMINGS))
        self.assertEqual({p for p, r in rows.items() if r['source'] == 'derived'}, set(sc.DERIVATION_RULES))
        self.assertEqual({p: str(r['value']) for p, r in rows.items()}, {
            'upgrade.min_notice_blocks': '720', 'release_handover.v2.min_notice_blocks': '720',
            'governance.deposit.deposit_period_blocks': '720', 'governance.ballot.voting_period_blocks': '720',
            'governance.ballot.timelock_blocks': '360', 'lifecycle.evidence_max_age_blocks': '2880',
            'lifecycle.processing_margin_blocks': '120', 'service.snapshots.interval_blocks': '720',
            'lifecycle.evidence_max_age_seconds': '14400', 'lifecycle.processing_margin_seconds': '600',
            'config.toml [statesync] trust_period': '2h', 'monitor.backup_max_age_seconds': '10800'})
        # 4 hours less a fourteenth: 3 h 42 min 51 s, above a backup's 3-hour freshness.
        self.assertEqual(record['restore_trust_seconds'], 13371)
        production = sc.read_json(sc.E05_VALUES)['values']
        approved = {r['path']: r.get('approved') for r in production}
        self.assertTrue(all(r['production'] == approved[p] for p, r in rows.items()))
        # Each value keeps its form (large integers as strings), and nothing else changes.
        self.assertEqual(sorted(b['path'] for _, b in self.changed(values)), sorted(rows))
        for before, after in self.changed(values):
            self.assertIs(type(before['approved']), type(after['approved']))
            self.assertEqual(after['approval_record'], sc.TIMINGS_APPROVAL)
        self.assertTrue((sc.MAINNET / sc.TIMINGS_APPROVAL).is_file())

    def test_violations_name_a_trust_period_or_a_lifecycle_too_long(self):
        chosen = dict(self.production(), **sc.REHEARSAL_TIMINGS)
        chosen.update(sc.derived(chosen, 5))
        chosen['config.toml [statesync] trust_period'] = '4h'
        chosen['governance.ballot.voting_period_blocks'] = 17280
        found = ' '.join(sc.violations(chosen, 5, 'rehearsal'))
        self.assertIn('trust period', found)
        self.assertIn('governance proposal', found)
        # The run-length rule is the rehearsal's only.
        self.assertEqual(sc.violations(self.production(), 5, 'production'), [])

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
