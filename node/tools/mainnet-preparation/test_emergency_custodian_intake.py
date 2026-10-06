"""Synthetic emergency custodian intakes exercise the checker. No real key, person or approval."""
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import emergency_custodian_intake as e
import upgrade_custodian_intake as u

STATEMENT = 'Synthetic structural test only. No real key, signature, appointment or approval.'
HERE = Path(__file__).resolve().parent
TEMPLATE = HERE.parents[2]/'launch'/'custody'/'emergency'/'PUBLIC_INTAKE.template.json'
FOUNDER = 'synthetic-founder'


def key_bytes(seed): return bytes([seed])*u.KEY_BYTES


class EmergencyIntakeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.build(solo=False)

    def build(self, solo):
        self.data = {'schema': e.SCHEMA, 'production_accepted': False, 'custody_model': 'solo_kits' if solo else 'independent',
                     'freeze_threshold': 3, 'resume_threshold': 3, 'authority_size': 5, 'authority_epoch': 1,
                     'profile': {'parameter_set': e.PARAMETER_SET, 'approval': 'profile'}, 'custodians': [], 'evidence': {}}
        self.evidence('profile', 'profile_approval')
        for i in range(5):
            c = FOUNDER if solo else f'synthetic-person-{i}'
            g = f'kit-{i+1}' if solo else f'synthetic-group-{i}'
            keys = {}
            for n, purpose in enumerate(e.PURPOSES):
                raw = key_bytes(1 + i*2 + n)
                key_id = hashlib.sha256(raw).hexdigest()
                key = {'key_id': key_id, 'public_key_base64': base64.b64encode(raw).decode(), 'signer_control_group': g, 'backup_control_group': g}
                for kind in e.KEY_EVIDENCE:
                    key[kind] = f'{i}-{purpose}-{kind}'
                    self.evidence(key[kind], kind, c, purpose, key_id, 1)
                keys[purpose] = key
            self.evidence(f'a{i}', 'appointment', c)
            review = None
            if not solo:
                review = f'i{i}'
                self.evidence(review, 'independence_review', c)
            self.data['custodians'].append({'slot': i+1, 'controller_id': c, 'name': c, 'organization': g, 'control_group': g,
                                            'appointment': f'a{i}', 'independence_review': review, 'keys': keys})

    def evidence(self, ref, kind, controller=None, purpose=None, key=None, epoch=None, reviewer='synthetic-independent-reviewer'):
        obj = {'kind': kind, 'controller_id': controller, 'purpose': purpose, 'key_id': key, 'parameter_set': e.PARAMETER_SET,
               'epoch': epoch, 'reviewer_control_group': reviewer, 'public_statement': STATEMENT}
        raw = json.dumps(obj).encode()
        (self.root/(ref+'.json')).write_bytes(raw)
        self.data['evidence'][ref] = {'path': ref+'.json', 'sha256': hashlib.sha256(raw).hexdigest()}

    def validate(self): return e.validate(self.data, self.root)

    def check_bad(self, message):
        result = self.validate()
        self.assertEqual(result['status'], 'INCOMPLETE_OR_INVALID')
        self.assertNotIn('authority_fragment', result)
        self.assertTrue(any(message in error for error in result['errors']), result['errors'])

    def key(self, slot=0, purpose='freeze'): return self.data['custodians'][slot]['keys'][purpose]

    def test_complete_is_only_structural(self):
        result = self.validate()
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['status'], 'STRUCTURALLY_COMPLETE')
        self.assertFalse(result['signature_verification_performed'])
        self.assertFalse(result['production_accepted'])
        self.assertEqual(result['custody_model'], 'independent')

    def test_the_fragment_is_the_genesis_records_shape(self):
        fragment = self.validate()['authority_fragment']
        # resolve_genesis_inputs.py takes records.root.emergency with exactly these fields.
        self.assertEqual(set(fragment), {'authority_epoch', 'freeze', 'resume'})
        self.assertEqual(fragment['authority_epoch'], 1)
        for purpose in e.PURPOSES:
            keys = fragment[purpose]['keys']
            self.assertEqual(fragment[purpose]['threshold'], 3)
            self.assertEqual(len(keys), 5)
            self.assertEqual([k['key_id'] for k in keys], sorted(k['key_id'] for k in keys))
            for k in keys:
                self.assertEqual(k['key_id'], hashlib.sha256(bytes.fromhex(k['public_key_hex'])).hexdigest())
        self.assertFalse({k['key_id'] for k in fragment['freeze']['keys']} & {k['key_id'] for k in fragment['resume']['keys']})

    def test_solo_kits(self):
        self.build(solo=True)
        result = self.validate()
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['custody_model'], 'solo_kits')

    def test_solo_kits_take_no_review(self):
        self.build(solo=True)
        self.data['custodians'][1]['independence_review'] = 'a0'
        self.check_bad('solo_kits has no independence review')

    def test_solo_kits_have_one_controller(self):
        self.build(solo=True)
        self.data['custodians'][3]['controller_id'] = 'someone-else'
        self.check_bad('one controller holds every slot')

    def test_solo_kits_are_distinct(self):
        self.build(solo=True)
        self.data['custodians'][4]['control_group'] = 'kit-1'
        self.check_bad('duplicate or missing control group')

    def test_independent_needs_distinct_controllers_and_review(self):
        self.data['custodians'][2]['controller_id'] = 'synthetic-person-0'
        self.check_bad('duplicate or missing controller')
        self.build(solo=False)
        self.data['custodians'][0]['independence_review'] = None
        self.check_bad('missing bound independence_review evidence')

    def test_freeze_and_resume_keys_differ(self):
        self.data['custodians'][0]['keys']['resume'] = dict(self.key(0, 'freeze'))
        self.check_bad('reused or missing key')

    def test_keys_are_not_shared_between_slots(self):
        self.data['custodians'][3]['keys']['freeze'] = dict(self.key(0, 'resume'))
        self.check_bad('reused or missing key')

    def test_key_identifier_is_the_digest(self):
        self.key(1, 'resume')['key_id'] = '0' * 64
        self.check_bad('SHA-256 of public bytes')

    def test_backup_stays_in_the_custodians_group(self):
        self.key(2)['backup_control_group'] = 'synthetic-group-3'
        self.check_bad('crosses custodian groups')

    def test_evidence_binds_its_purpose(self):
        freeze, resume = self.key(0, 'freeze'), self.key(0, 'resume')
        freeze['proof_of_possession'], resume['proof_of_possession'] = resume['proof_of_possession'], freeze['proof_of_possession']
        self.check_bad('subject, purpose, key, epoch or profile mismatch')

    def test_fixed_values(self):
        for field, value in (('freeze_threshold', 2), ('resume_threshold', 4), ('authority_size', 7)):
            self.build(solo=False)
            self.data[field] = value
            self.check_bad('approved value mismatch')
        self.build(solo=False)
        self.data['authority_epoch'] = 0
        self.check_bad('positive authority epoch')
        self.build(solo=False)
        self.data['schema'] = 'dytallix.emergency-custodian-intake.v1'
        self.check_bad('schema mismatch')

    def test_unreferenced_evidence(self):
        self.evidence('extra', 'appointment', 'synthetic-person-0')
        self.check_bad('unreferenced evidence')

    def test_the_upgrade_and_genesis_checkers_read_it(self):
        self.assertEqual(u.EMERGENCY_SCHEMA, e.SCHEMA)
        held, problem = u.emergency_holdings(self.data)
        self.assertIsNone(problem)
        controllers, groups, keys = held
        self.assertEqual(len(controllers), 5)
        self.assertEqual(len(keys), 10)

    def test_the_template_is_incomplete(self):
        run = subprocess.run([sys.executable, '-B', str(HERE/'emergency_custodian_intake.py'), str(TEMPLATE)],
                             capture_output=True, text=True)
        self.assertEqual(run.returncode, 2)
        result = json.loads(run.stdout)
        self.assertEqual(result['status'], 'INCOMPLETE_OR_INVALID')
        template = json.loads(TEMPLATE.read_text())
        self.assertEqual(set(template), e.TOP)
        self.assertEqual(template['schema'], e.SCHEMA)


if __name__ == '__main__': unittest.main(verbosity=2)
