"""LAUNCH_GATES.json's decision copies follow the register. No decision or gate is accepted."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import decision_copies as d

HERE = Path(__file__).resolve().parent


class DecisionCopiesTests(unittest.TestCase):
    def setUp(self):
        self.gates = json.loads(d.GATES.read_text(encoding='utf-8'))
        self.register = json.loads(d.REGISTER.read_text(encoding='utf-8'))

    def question(self, qid): return d.questions(self.register)[qid]

    def test_the_committed_copies_follow_the_register(self):
        found = d.differences(self.gates, self.register)
        if found:
            self.fail(f'{len(found)} differences; run tools/mainnet-preparation/decision_copies.py --write\n' + '\n'.join(found[:20]))
        # Every gate depends on at least one register question.
        self.assertEqual(len(self.gates['gates']), 35)
        for gate in self.gates['gates']:
            self.assertTrue(any(c['source_ref'] == d.SOURCE_REF for c in gate['decision_dependencies']), gate['id'])

    def test_the_files_are_in_canonical_form(self):
        for path in (d.GATES, d.REGISTER):
            raw = path.read_text(encoding='utf-8')
            self.assertEqual(d.render(json.loads(raw)), raw, path.name)

    def test_an_approval_in_the_register_must_reach_every_copy(self):
        self.question('D12-Q02').update(status='APPROVED', approval_record='approvals/test.json')
        found = d.differences(self.gates, self.register)
        holders = [g['id'] for g in self.gates['gates'] if any(c['id'] == 'D12-Q02' for c in g['decision_dependencies'])]
        self.assertTrue(holders)
        for field in ('status', 'approval_record'):
            self.assertEqual(sorted(line.split('.')[0] for line in found if f'D12-Q02 {field} ' in line),
                             sorted(f'gates[{g}]' for g in holders), field)
        # The counts no longer count the register's questions either.
        self.assertTrue(any('do not count its questions' in line for line in found))

    def test_changed_question_text_is_reported(self):
        self.question('D13-Q02')['question'] += ' (reworded)'
        self.question('D13-Q02')['blocking_output'] = 'Something else'
        found = d.differences(self.gates, self.register)
        self.assertTrue(any('D13-Q02 question ' in line for line in found))
        self.assertTrue(any('D13-Q02 blocking_output ' in line for line in found))

    def test_an_unknown_decision_is_reported(self):
        first = next(c for _, c in d.copies(self.gates))
        first['id'] = 'D99-Q01'
        self.assertTrue(any("'D99-Q01' is not in the register" in line for line in d.differences(self.gates, self.register)))

    def test_stale_counts_are_reported(self):
        self.gates['decision_counts']['fully_open_policy_questions'] += 1
        self.assertTrue(any(line.startswith('decision_counts is') for line in d.differences(self.gates, self.register)))

    def test_sync_changes_only_the_copies_and_counts(self):
        original = copy.deepcopy(self.gates)
        for _, c in d.copies(self.gates): c.update(status='OPEN', approval_record=None, question='stale')
        self.gates['decision_counts'] = {'fully_open_policy_questions': 20}
        self.assertTrue(d.differences(self.gates, self.register))
        d.sync(self.gates, self.register)
        self.assertEqual(d.differences(self.gates, self.register), [])
        self.assertEqual(self.gates, original)
        self.assertEqual({g['status'] for g in self.gates['gates']}, {'PARTIAL'})

    def test_the_cli(self):
        command = [sys.executable, '-B', str(HERE/'decision_copies.py')]
        done = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual((done.returncode, json.loads(done.stdout)['status']), (0, 'IN_SYNC'), done.stderr)
        with tempfile.TemporaryDirectory() as tmp:
            gates, register = Path(tmp)/'LAUNCH_GATES.json', Path(tmp)/'MAINNET_DECISION_REGISTER.json'
            register.write_text(d.render(self.register), encoding='utf-8')
            stale = copy.deepcopy(self.gates)
            next(c for _, c in d.copies(stale))['status'] = 'OPEN'
            gates.write_text(d.render(stale), encoding='utf-8')
            paths = ['--gates', str(gates), '--register', str(register)]
            drift = subprocess.run(command + paths, capture_output=True, text=True)
            self.assertEqual((drift.returncode, json.loads(drift.stdout)['status']), (1, 'DRIFT'))
            written = subprocess.run(command + paths + ['--write'], capture_output=True, text=True)
            self.assertEqual(written.returncode, 0, written.stderr)
            self.assertEqual(gates.read_text(encoding='utf-8'), d.render(self.gates))
            # A file in another layout is not rewritten.
            gates.write_text(json.dumps(stale), encoding='utf-8')
            refused = subprocess.run(command + paths + ['--write'], capture_output=True, text=True)
            self.assertEqual(refused.returncode, 2)
            self.assertIn('canonical form', refused.stderr)
            self.assertEqual(gates.read_text(encoding='utf-8'), json.dumps(stale))


if __name__ == '__main__': unittest.main(verbosity=2)
