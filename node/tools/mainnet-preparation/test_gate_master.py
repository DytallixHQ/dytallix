"""The readable gate list follows LAUNCH_GATES.json. No gate is accepted."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import unittest
import gate_master as m

HERE = Path(__file__).resolve().parent


class GateMasterTests(unittest.TestCase):
    def setUp(self):
        self.data = json.loads(m.GATES.read_text(encoding='utf-8'))
        self.master = m.MASTER.read_text(encoding='utf-8')

    def gate(self, gid): return next(g for g in self.data['gates'] if g['id'] == gid)

    def found(self, data=None): return m.problems(data or self.data, m.render(data or self.data))

    def test_the_committed_list_is_current(self):
        found = m.problems(self.data, self.master)
        self.assertEqual(found, [], 'run tools/mainnet-preparation/gate_master.py --write')

    def test_outside_records_are_text_and_repository_files_are_links(self):
        self.assertNotIn('](decision-register/', self.master)
        self.assertNotIn('](batch-', self.master)
        self.assertIn('- `decision-register/', self.master)
        self.assertIn('- [USER_LAUNCH_REQUIREMENTS.txt lines ', self.master)

    def test_the_solo_profile_comes_from_the_data(self):
        for text in self.data['acceptance_profile']['terms'].values(): self.assertIn(text, self.master)

    def test_an_edit_to_the_data_makes_the_list_stale(self):
        self.gate('G27')['remaining_work'].append('A new step.')
        found = m.problems(self.data, self.master)
        self.assertEqual(len(found), 1)
        self.assertIn('stale', found[0])

    def test_counts_follow_the_gates(self):
        self.gate('G01')['status'] = 'OPEN'
        self.assertIn('status_counts', self.found()[0])
        self.data['status_counts'].update(OPEN=1, PARTIAL=34)
        self.assertEqual(self.found(), [])

    def test_launch_stays_no_go_until_everything_is_complete(self):
        self.data['status'] = 'GO'
        self.assertTrue(any('NO_GO' in p for p in self.found()))
        self.data['status'], self.data['production_activation_authorized'] = 'NO_GO', True
        self.assertTrue(any('NO_GO' in p for p in self.found()))

    def test_a_passed_gate_has_no_remaining_work_or_blocker(self):
        self.gate('G33')['status'] = 'PASS'
        self.data['status_counts'].update(PARTIAL=34, PASS=1)
        self.data.update(passed_count=1, readiness_percent=100 / 35)
        found = self.found()
        self.assertIn('G33: PASS with remaining work', found)
        self.assertIn('G33: PASS with a blocker', found)

    def test_g35_keeps_its_veto(self):
        self.gate('G35')['launch_veto'] = False
        self.assertIn('G35 must stay mandatory with its launch veto', self.found())

    def test_repository_sources_must_exist_and_match_their_pins(self):
        refs = self.gate('G01')['requirement_refs']
        pinned = next(e for e in refs if 'sha256' in e and not m.outside(e['path']))
        pinned['sha256'] = '0' * 64
        self.assertIn(f'G01: {pinned["path"]} differs from its pinned digest', self.found())
        pinned['line_end'] = 10 ** 6
        self.assertTrue(any('has no lines' in p for p in self.found()))
        refs.append({'path': 'NO_SUCH_FILE.md'})
        self.assertIn('G01: NO_SUCH_FILE.md is not in this repository', self.found())

    def test_records_held_outside_are_not_looked_up(self):
        data = copy.deepcopy(self.data)
        data['gates'][0]['evidence'].append({'path': 'decision-register/none/RECORD.json', 'sha256': '0' * 64})
        self.assertEqual(self.found(data), [])

    def test_the_gates_are_g01_to_g35_in_order(self):
        self.data['gates'].reverse()
        self.assertEqual(self.found(), ['the gates must be exactly G01 to G35, in order'])

    def test_the_cli_checks_the_committed_files(self):
        done = subprocess.run([sys.executable, '-B', str(HERE/'gate_master.py')], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(json.loads(done.stdout)['status'], 'PASS')


if __name__ == '__main__': unittest.main()
