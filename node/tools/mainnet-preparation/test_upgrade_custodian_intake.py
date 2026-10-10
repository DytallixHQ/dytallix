"""Synthetic upgrade custodian intakes exercise the checker. No real key, person or approval."""
import base64
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import upgrade_custodian_intake as u

STATEMENT = 'Synthetic structural test only. No real key, signature, appointment or approval.'
TEMPLATE = Path(__file__).resolve().parents[3]/'launch'/'custody'/'upgrade'/'PUBLIC_INTAKE.template.json'


def key_bytes(seed): return bytes([seed])*u.KEY_BYTES


FOUNDER = 'synthetic-founder'


def emergency_packet(solo=False):
    people = []
    for i in range(5):
        g = f'synthetic-kit-{i}' if solo else f'synthetic-emergency-group-{i}'
        keys = {}
        for n, purpose in enumerate(('freeze', 'resume')):
            raw = key_bytes(100+i*2+n)
            keys[purpose] = {'key_id': hashlib.sha256(raw).hexdigest(), 'public_key_base64': base64.b64encode(raw).decode(), 'epoch': 1, 'signer_control_group': g, 'backup_control_group': g}
        people.append({'slot': i+1, 'controller_id': FOUNDER if solo else f'synthetic-emergency-{i}', 'control_group': g, 'keys': keys})
    return {'schema': u.EMERGENCY_SCHEMA, 'custodians': people}


class UpgradeIntakeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.emergency = emergency_packet()
        self.data = {'schema': u.SCHEMA, 'production_accepted': False, 'custody_model': 'independent', 'threshold': 3, 'authority_size': 5, 'authority_epoch': 1,
                     'profile': {'parameter_set': u.PARAMETER_SET, 'approval': 'profile'}, 'custodians': [], 'evidence': {}}
        self.evidence('profile', 'profile_approval')
        for i in range(5):
            c, g = f'synthetic-person-{i}', f'synthetic-group-{i}'
            raw = key_bytes(i+1)
            key_id = hashlib.sha256(raw).hexdigest()
            key = {'key_id': key_id, 'public_key_base64': base64.b64encode(raw).decode(), 'signer_control_group': g, 'backup_control_group': g}
            for kind in u.KEY_EVIDENCE:
                key[kind] = f'{i}-{kind}'
                self.evidence(key[kind], kind, c, u.PURPOSE, key_id, 1)
            self.evidence(f'a{i}', 'appointment', c)
            self.evidence(f'i{i}', 'independence_review', c)
            self.data['custodians'].append({'slot': i+1, 'controller_id': c, 'name': c, 'organization': g, 'control_group': g,
                                            'appointment': f'a{i}', 'independence_review': f'i{i}', 'key': key})

    def evidence(self, ref, kind, controller=None, purpose=None, key=None, epoch=None, reviewer='synthetic-independent-reviewer'):
        obj = {'kind': kind, 'controller_id': controller, 'purpose': purpose, 'key_id': key, 'parameter_set': u.PARAMETER_SET,
               'epoch': epoch, 'reviewer_control_group': reviewer, 'public_statement': STATEMENT}
        raw = json.dumps(obj).encode()
        (self.root/(ref+'.json')).write_bytes(raw)
        self.data['evidence'][ref] = {'path': ref+'.json', 'sha256': hashlib.sha256(raw).hexdigest()}

    def validate(self): return u.validate(self.data, self.root, self.emergency)

    def check_bad(self, message):
        result = self.validate()
        self.assertEqual(result['status'], 'INCOMPLETE_OR_INVALID')
        self.assertNotIn('authority_fragment', result)
        self.assertTrue(any(message in error for error in result['errors']), result['errors'])

    def person(self, slot=0): return self.data['custodians'][slot]

    def key(self, slot=0): return self.person(slot)['key']

    def test_complete_is_only_structural(self):
        result = self.validate()
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['status'], 'STRUCTURALLY_COMPLETE')
        self.assertFalse(result['signature_verification_performed'])
        self.assertFalse(result['identity_or_independence_verified'])
        self.assertFalse(result['production_accepted'])

    def test_authority_fragment_matches_the_node_shape(self):
        fragment = self.validate()['authority_fragment']
        keys = fragment['authority']['keys']
        self.assertEqual(fragment['authority']['threshold'], 3)
        self.assertEqual(fragment['authority_epoch'], 1)
        self.assertEqual(fragment['parameter_set'], 'SLH-DSA-SHAKE-256s')
        self.assertEqual(len(keys), 5)
        self.assertEqual([k['key_id'] for k in keys], sorted(k['key_id'] for k in keys))
        for k in keys:
            self.assertEqual(set(k), {'key_id', 'public_key_hex'})
            self.assertEqual(k['key_id'], hashlib.sha256(bytes.fromhex(k['public_key_hex'])).hexdigest())

    def test_no_acceptance_override(self):
        self.data['production_accepted'] = True; self.check_bad('acceptance')

    def test_exact_threshold(self):
        self.data['threshold'] = 2; self.check_bad('approved value')

    def test_exact_size(self):
        self.data['authority_size'] = 7; self.check_bad('approved value')

    def test_exact_five(self):
        self.data['custodians'].pop(); self.check_bad('exactly five')

    def test_unapproved_parameter_set(self):
        self.data['profile']['parameter_set'] = 'SLH-DSA-SHA2-256s'; self.check_bad('approved SLH-DSA-SHAKE-256s')

    def test_unset_parameter_set(self):
        self.data['profile']['parameter_set'] = None; self.check_bad('approved SLH-DSA-SHAKE-256s')

    def test_missing_epoch(self):
        self.data['authority_epoch'] = None; self.check_bad('positive authority epoch')

    def test_zero_epoch(self):
        self.data['authority_epoch'] = 0; self.check_bad('positive authority epoch')

    def test_epoch_rebinding(self):
        self.data['authority_epoch'] = 2; self.check_bad('subject, purpose, key, epoch or profile mismatch')

    def test_duplicate_controller(self):
        self.person(1)['controller_id'] = 'synthetic-person-0'; self.check_bad('duplicate or missing controller')

    def test_shared_control(self):
        self.person(1)['control_group'] = 'synthetic-group-0'; self.check_bad('duplicate or missing control group')

    def test_duplicate_slot(self):
        self.person(1)['slot'] = 1; self.check_bad('distinct slots')

    def test_missing_name(self):
        self.person()['name'] = None; self.check_bad('name missing')

    def test_key_reuse_across_custodians(self):
        self.person(1)['key'] = copy.deepcopy(self.key()); self.check_bad('reused or missing key')

    def test_malformed_key(self):
        self.key()['public_key_base64'] = '!!'; self.check_bad('invalid public-key')

    def test_wrong_key_size(self):
        self.key()['public_key_base64'] = base64.b64encode(b'x'*32).decode(); self.check_bad('invalid public-key')

    def test_zero_key(self):
        self.key()['public_key_base64'] = base64.b64encode(bytes(u.KEY_BYTES)).decode(); self.check_bad('invalid public-key')

    def test_fingerprint_mismatch(self):
        self.key()['key_id'] = 'a'*64; self.check_bad('key identifier')

    def test_cross_custodian_evidence(self):
        self.key()['proof_of_possession'] = self.key(1)['proof_of_possession']; self.check_bad('subject, purpose, key, epoch or profile mismatch')

    def test_wrong_purpose(self):
        self.evidence('0-drill_record', 'drill_record', 'synthetic-person-0', 'freeze', self.key()['key_id'], 1)
        self.check_bad('subject, purpose, key, epoch or profile mismatch')

    def test_missing_evidence(self):
        self.key()['proof_of_possession'] = None; self.check_bad('missing bound evidence')

    def test_tampered_evidence(self):
        (self.root/'profile.json').write_text('{}'); self.check_bad('digest mismatch')

    def test_duplicate_json_key(self):
        raw = b'{"kind": "profile_approval", "kind": "appointment"}'
        (self.root/'profile.json').write_bytes(raw)
        self.data['evidence']['profile']['sha256'] = hashlib.sha256(raw).hexdigest(); self.check_bad('UTF-8 JSON')

    def test_path_escape(self):
        self.data['evidence']['profile']['path'] = '../profile.json'; self.check_bad('unsafe path')

    def test_symlink(self):
        (self.root/'link.json').symlink_to(self.root/'profile.json'); self.data['evidence']['profile']['path'] = 'link.json'; self.check_bad('symlink')

    def test_unknown_secret_field(self):
        self.key()['private_key'] = 'not a real secret'; self.check_bad('exact fields')

    def test_shared_backup_control(self):
        self.key()['backup_control_group'] = 'synthetic-group-1'; self.check_bad('backup control crosses')

    def test_reviewer_control(self):
        self.evidence('i0', 'independence_review', 'synthetic-person-0', reviewer='synthetic-group-1'); self.check_bad('reviewer shares')

    def test_self_review(self):
        self.evidence('i0', 'independence_review', 'synthetic-person-0', reviewer='synthetic-group-0'); self.check_bad('self-review')

    def test_unreferenced_evidence(self):
        self.evidence('extra', 'appointment'); self.check_bad('unreferenced')

    def test_emergency_intake_required(self):
        self.emergency = None; self.check_bad('emergency intake required')

    def test_incomplete_emergency_intake(self):
        self.emergency['custodians'][2]['keys']['resume']['public_key_base64'] = None; self.check_bad('emergency intake incomplete')

    def test_wrong_emergency_schema(self):
        self.emergency['schema'] = u.SCHEMA; self.check_bad('emergency intake: schema mismatch')

    def test_emergency_controller(self):
        self.person()['controller_id'] = 'synthetic-emergency-3'; self.check_bad('also an emergency custodian')

    def test_emergency_control_group(self):
        self.emergency['custodians'][4]['control_group'] = 'synthetic-group-0'; self.check_bad('shared with an emergency custodian')

    def test_emergency_backup_group(self):
        self.emergency['custodians'][4]['keys']['freeze']['backup_control_group'] = 'synthetic-group-2'; self.check_bad('shared with an emergency custodian')

    def test_emergency_key(self):
        # The node's configuration check refuses this too (consensus_settlement.rs).
        self.emergency['custodians'][1]['keys']['freeze']['public_key_base64'] = self.key()['public_key_base64']
        self.check_bad('freeze or resume role')

    def make_solo(self):
        """The solo launch profile: the founder holds every slot, kit N in slot N."""
        self.data['custody_model'] = 'solo_kits'
        self.emergency = emergency_packet(solo=True)
        for i, person in enumerate(self.data['custodians']):
            kit = f'synthetic-kit-{i}'
            person.update(controller_id=FOUNDER, name=FOUNDER, organization=FOUNDER, control_group=kit, independence_review=None)
            del self.data['evidence'][f'i{i}']
            (self.root/f'i{i}.json').unlink()
            self.evidence(f'a{i}', 'appointment', FOUNDER)
            key = person['key']
            key.update(signer_control_group=kit, backup_control_group=kit)
            for kind in u.KEY_EVIDENCE:
                self.evidence(key[kind], kind, FOUNDER, u.PURPOSE, key['key_id'], 1)

    def test_solo_kits_complete(self):
        self.make_solo()
        result = self.validate()
        self.assertEqual(result['errors'], [])
        self.assertEqual((result['status'], result['custody_model']), ('STRUCTURALLY_COMPLETE', 'solo_kits'))
        self.assertEqual(len(result['authority_fragment']['authority']['keys']), 5)

    def test_solo_kits_rules(self):
        for change, message in (
            (lambda: self.person(2).update(controller_id='someone-else'), 'one controller holds every slot'),
            (lambda: self.person(1).update(control_group='synthetic-kit-0'), 'duplicate or missing control group'),
            (lambda: self.person(0).update(independence_review='i0'), 'has no independence review'),
            (lambda: self.emergency['custodians'][0].update(controller_id='someone-else'), 'emergency intake names another controller'),
            (lambda: self.emergency['custodians'][3].update(control_group='synthetic-kit-9'), 'emergency keys use other kits'),
            (lambda: self.emergency['custodians'][1]['keys']['freeze'].update(public_key_base64=self.key()['public_key_base64']), 'freeze or resume role'),
        ):
            self.setUp(); self.make_solo(); change()
            self.check_bad(message)

    def test_custody_model_is_required(self):
        for model in (None, 'shared'):
            self.setUp(); self.data['custody_model'] = model
            self.check_bad('custody_model must be independent, solo_kits or five_holders')
        # An independent packet still refuses one controller in two slots.
        self.setUp(); self.person(1).update(controller_id=self.person(0)['controller_id'])
        self.check_bad('duplicate or missing controller')

    def test_template_is_incomplete(self):
        template = json.loads(TEMPLATE.read_text())
        result = u.validate(template, TEMPLATE.parent, self.emergency)
        self.assertEqual(result['status'], 'INCOMPLETE_OR_INVALID')
        self.assertEqual(set(template), u.TOP)
        self.assertTrue(all(set(p) == u.PERSON and set(p['key']) == u.KEY for p in template['custodians']))

    def test_command_line(self):
        packet = self.root/'packet.json'; packet.write_text(json.dumps(self.data))
        emergency = self.root/'emergency.json'; emergency.write_text(json.dumps(self.emergency))
        tool = Path(u.__file__).resolve()
        done = subprocess.run([sys.executable, '-B', str(tool), str(packet), '--emergency', str(emergency)], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stdout+done.stderr)
        done = subprocess.run([sys.executable, '-B', str(tool), str(packet)], capture_output=True, text=True)
        self.assertEqual(done.returncode, 2)
        self.assertEqual(json.loads(done.stdout)['status'], 'INCOMPLETE_OR_INVALID')


if __name__ == '__main__': unittest.main(verbosity=2)
