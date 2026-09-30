"""The genesis input resolver and the synthetic rehearsal records (E05-d). No production input is selected."""
import copy
import json
from pathlib import Path
import unittest
import genesis_rehearsal_records as records_module
import resolve_genesis_inputs as r

HERE = Path(__file__).resolve().parent
LAUNCH = HERE.parents[2]/'launch'
REHEARSAL = HERE/'fixtures'/'genesis-rehearsal'
NOT_GENESIS = ('config.toml', 'service.', 'emergency verifier', 'config/pqc', '(')


class ResolverTests(unittest.TestCase):
    def setUp(self):
        self.values = json.loads((LAUNCH/'E05_VALUES.json').read_text())
        self.proposals = json.loads((LAUNCH/'genesis'/'PROPOSALS.json').read_text())
        self.records = json.loads((REHEARSAL/'records.json').read_text())

    def resolve(self): return r.resolve(self.values, self.proposals, self.records)

    def refused(self, message):
        with self.assertRaises(ValueError) as caught: self.resolve()
        self.assertIn(message, str(caught.exception))

    def entry(self, name): return next(v for v in self.values['values'] if v['name'] == name)

    def test_the_committed_rehearsal_is_current(self):
        inputs, report = self.resolve()
        self.assertEqual(r.render(inputs), (REHEARSAL/'inputs.json').read_text(), 'rerun resolve_genesis_inputs.py')
        self.assertEqual(r.render(report), (REHEARSAL/'resolution.json').read_text(), 'rerun resolve_genesis_inputs.py')

    def test_the_synthetic_records_are_reproducible(self):
        text = json.dumps(records_module.records(), indent=2, sort_keys=True) + '\n'
        self.assertEqual(text, (REHEARSAL/'records.json').read_text(), 'rerun genesis_rehearsal_records.py')

    def test_a_rehearsal_is_never_production_eligible(self):
        _, report = self.resolve()
        self.assertFalse(report['production_eligible'])
        self.assertEqual(self.records['status'], 'SYNTHETIC')
        self.assertTrue(all(p['status'] in r.PROPOSAL_STATUSES for p in self.proposals['values'].values()))

    def test_approved_values_are_used_as_approved(self):
        inputs, report = self.resolve()
        self.assertEqual(inputs['ordinary']['gas_price'], 10)
        self.assertEqual(inputs['lifecycle']['min_self_bond'], '100000000000')
        self.assertEqual(inputs['issuance']['controller']['soft']['proportional'], 17280000000000)
        source = {v['name']: v['source'] for v in report['values']}
        self.assertEqual(source['ordinary_minimum_gas'], 'APPROVED')
        self.assertEqual(source['queue_max_entries'], 'MEASURE_PLACEHOLDER')

    def test_every_genesis_value_is_resolved_or_derived(self):
        for v in self.values['values']:
            if v['path'].startswith(NOT_GENESIS): continue
            if v['tier'] == 'derived' and v['name'] not in r.VALUES: continue
            self.assertIn(v['name'], r.VALUES, f'{v["name"]} ({v["path"]}) has no place in the build inputs')

    def test_a_proposal_for_an_approved_value_is_refused(self):
        self.proposals['values']['min_self_bond'] = {'value': '1', 'status': 'PROPOSED', 'basis': 'stale'}
        self.refused('proposals for approved values')

    def test_an_open_value_needs_a_proposal(self):
        del self.proposals['values']['app_max_txs']
        self.refused('app_max_txs: open with no proposal')

    def test_unknown_and_unused_names_are_refused(self):
        self.proposals['values']['no_such_value'] = {'value': 1, 'status': 'PROPOSED', 'basis': 'x'}
        self.refused('unknown values')
        del self.proposals['values']['no_such_value']
        self.proposals['values']['statesync_max_snapshot_chunks'] = {'value': 1, 'status': 'PROPOSED', 'basis': 'x'}
        self.refused('does not use')

    def test_a_proposal_carries_a_label(self):
        self.proposals['values']['app_max_txs']['status'] = 'APPROVED'
        self.refused('proposal status')

    def test_values_keep_their_types(self):
        self.proposals['values']['ordinary_max_actions']['value'] = 'sixteen'
        self.refused('ordinary_max_actions')
        self.proposals['values']['ordinary_max_actions']['value'] = 16
        self.entry('ordinary_action_costs')['approved'] = [5000] * 11
        self.refused('ordinary_action_costs')

    def test_records_are_synthetic_or_accepted(self):
        self.records['status'] = 'DRAFT'
        self.refused('records status')

    def test_the_upgrade_authority_uses_the_approved_parameter_set(self):
        self.records['root']['upgrade']['parameter_set'] = 'SLH-DSA-SHA2-256s'
        self.refused('SLH-DSA-SHAKE-256s')

    def test_records_have_exact_fields(self):
        extra = copy.deepcopy(self.records)
        extra['operator_contacts'] = []
        self.records = extra
        self.refused('exact fields')


if __name__ == '__main__': unittest.main(verbosity=2)
