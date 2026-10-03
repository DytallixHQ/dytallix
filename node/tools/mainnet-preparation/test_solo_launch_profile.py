"""The solo launch profile (P01, 3 October 2026) is recorded consistently. Approves nothing."""
import json
from pathlib import Path
import unittest

LAUNCH = Path(__file__).resolve().parents[3]/'launch'
SOLO = 'approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json'


def load(name): return json.loads((LAUNCH/name).read_text())


class SoloLaunchProfileTests(unittest.TestCase):
    def test_every_record_names_the_same_approval_and_disclosure(self):
        approval = load(SOLO)
        self.assertEqual(len(approval['decisions']), 11)
        register, gates = load('MAINNET_DECISION_REGISTER.json'), load('LAUNCH_GATES.json')
        self.assertEqual(register['launch_profile']['approval_record'], SOLO)
        self.assertEqual(gates['acceptance_profile']['approval_record'], SOLO)
        self.assertEqual(register['launch_profile']['id'], gates['acceptance_profile']['id'])
        for profile in (register['launch_profile'], gates['acceptance_profile']):
            self.assertTrue((LAUNCH/profile['disclosure_ref']).is_file())

    def test_each_decided_question_carries_the_profile(self):
        approval = load(SOLO)
        decided = {q.strip() for d in approval['decisions'] for q in d['question'].split(',') if q.strip().startswith('D')}
        register = load('MAINNET_DECISION_REGISTER.json')
        questions = {q['id']: q for c in register['categories'] for q in c['questions']}
        for qid in decided:
            q = questions[qid]
            refs = [q.get('solo_launch_profile_ref'), q.get('approval_record')] + q.get('approval_records', [])
            self.assertIn(SOLO, refs, qid)

    def test_the_topology_and_objectives_match(self):
        infra = load('PRODUCTION_INFRASTRUCTURE_DRAFT.json')['candidate_architecture']
        self.assertEqual((infra['launch_profile']['hosts'], infra['validators']['count'], infra['sentries']['count_per_validator']), (3, 1, 1))
        objectives = load('operations/OBJECTIVES.json')
        self.assertEqual(objectives['approval_record'], SOLO)
        self.assertEqual(objectives['recovery']['rto_minutes']['endpoint_service'], 240)

    def test_the_disclosure_states_the_concentrations(self):
        text = (LAUNCH/'TRUST_MODEL.md').read_text()
        for phrase in ('unaudited', '100% of DGT', 'five key kits', 'One validator'):
            self.assertIn(phrase, text)


if __name__ == '__main__': unittest.main(verbosity=2)
