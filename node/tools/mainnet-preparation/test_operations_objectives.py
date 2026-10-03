"""The approved operations objectives (D12-Q02) agree with the approved E05 values."""
import json
from pathlib import Path
import unittest

LAUNCH = Path(__file__).resolve().parents[3]/'launch'


class OperationsObjectivesTests(unittest.TestCase):
    def setUp(self):
        self.objectives = json.loads((LAUNCH/'operations'/'OBJECTIVES.json').read_text())
        values = json.loads((LAUNCH/'E05_VALUES.json').read_text())
        self.block_seconds = values['block_interval_assumption_seconds']
        self.values = {v['name']: v for v in values['values']}

    def approved(self, name):
        entry = self.values[name]
        self.assertEqual(entry['status'], 'APPROVED', name)
        return entry['approved']

    def test_the_record_is_the_approved_one(self):
        self.assertEqual(self.objectives['schema'], 'dytallix.operations-objectives.v1')
        approval = json.loads((LAUNCH/self.objectives['approval_record']).read_text())
        self.assertEqual([d['question'] for d in approval['decisions']], ['D12-Q02'] * 4)

    def test_the_validator_rto_covers_provisioning_and_the_approved_catch_up(self):
        recovery = self.objectives['recovery']
        catch_up_minutes = self.approved('catch_up_millis') / 60_000
        self.assertEqual(recovery['validator_rto_basis']['provision_minutes'] + catch_up_minutes, recovery['rto_minutes']['validator'])
        self.assertEqual(recovery['rpo_committed_blocks'], 0)

    def test_backups_follow_the_approved_daily_snapshots(self):
        # One snapshot a day, three kept, so the off-host copy is at most a day old.
        self.assertEqual(self.approved('snapshot_interval_blocks') * self.block_seconds, 24 * 3600)
        self.assertEqual(self.approved('snapshot_keep'), 3)
        self.assertEqual(self.objectives['backup']['encryption'], 'AES-256')

    def test_archive_nodes_keep_full_history_beside_the_retained_window(self):
        self.assertEqual(self.approved('block_history'), 'window')
        history = self.objectives['retention']['chain_history']
        self.assertGreaterEqual(history['archive_nodes_min'], 2)
        self.assertTrue(history['archive_nodes_distinct_operators'] and history['archive_nodes_distinct_providers'])

    def test_the_monthly_budgets(self):
        targets = self.objectives['service_targets']
        minutes = 30 * 24 * 60
        self.assertAlmostEqual(minutes * (100 - targets['chain']['target_percent']) / 100, 43.2)
        self.assertAlmostEqual(minutes * (100 - targets['endpoints']['target_percent']) / 100, 216)
        self.assertEqual(self.objectives['on_call']['severities'][0]['acknowledge_minutes'], 15)


if __name__ == '__main__': unittest.main(verbosity=2)
