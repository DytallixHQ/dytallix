"""Synthetic genesis signer intakes exercise the checker. No real key, person or approval."""
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import genesis_signer_intake as g
import upgrade_custodian_intake as u
from test_upgrade_custodian_intake import emergency_packet

STATEMENT = 'Synthetic structural test only. No real key, signature, appointment or approval.'
TEMPLATE = Path(__file__).resolve().parents[3]/'launch'/'custody'/'genesis'/'PUBLIC_INTAKE.template.json'
CHAIN = 'dytallix-staging-1'


def key_bytes(seed): return bytes([seed])*g.KEY_BYTES


def upgrade_packet():
    people = []
    for i in range(5):
        group = f'synthetic-upgrade-group-{i}'
        raw = key_bytes(150+i)
        people.append({'slot': i+1, 'controller_id': f'synthetic-upgrade-{i}', 'control_group': group,
                       'key': {'key_id': hashlib.sha256(raw).hexdigest(), 'public_key_base64': base64.b64encode(raw).decode(),
                               'signer_control_group': group, 'backup_control_group': group}})
    return {'schema': u.SCHEMA, 'custodians': people}


class GenesisSignerIntakeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.emergency, self.upgrade = emergency_packet(), upgrade_packet()
        self.data = {'schema': g.SCHEMA, 'production_accepted': False, 'threshold': 3, 'authority_size': 5, 'chain_id': CHAIN,
                     'profile': {'parameter_set': g.PARAMETER_SET, 'approval': 'profile'}, 'signers': [], 'evidence': {}}
        self.evidence('profile', 'profile_approval')
        for i in range(5):
            c, grp = f'synthetic-signer-{i}', f'synthetic-signer-group-{i}'
            raw = key_bytes(i+1)
            key_id = hashlib.sha256(raw).hexdigest()
            key = {'key_id': key_id, 'public_key_hex': raw.hex(), 'signer_control_group': grp, 'backup_control_group': grp}
            for kind in u.KEY_EVIDENCE:
                key[kind] = f'{i}-{kind}'
                self.evidence(key[kind], kind, c, g.PURPOSE, key_id)
            self.evidence(f'a{i}', 'appointment', c)
            self.evidence(f'i{i}', 'independence_review', c)
            self.data['signers'].append({'slot': i+1, 'controller_id': c, 'name': c, 'organization': grp, 'control_group': grp,
                                         'appointment': f'a{i}', 'independence_review': f'i{i}', 'key': key})

    def evidence(self, ref, kind, controller=None, purpose=None, key=None, epoch=None, reviewer='synthetic-independent-reviewer'):
        obj = {'kind': kind, 'controller_id': controller, 'purpose': purpose, 'key_id': key, 'parameter_set': g.PARAMETER_SET,
               'epoch': epoch, 'reviewer_control_group': reviewer, 'public_statement': STATEMENT}
        raw = json.dumps(obj).encode()
        (self.root/(ref+'.json')).write_bytes(raw)
        self.data['evidence'][ref] = {'path': ref+'.json', 'sha256': hashlib.sha256(raw).hexdigest()}

    def validate(self): return g.validate(self.data, self.root, self.emergency, self.upgrade)

    def check_bad(self, message):
        result = self.validate()
        self.assertEqual(result['status'], 'INCOMPLETE_OR_INVALID')
        self.assertNotIn('signer_policy', result)
        self.assertTrue(any(message in error for error in result['errors']), result['errors'])

    def person(self, slot=0): return self.data['signers'][slot]

    def key(self, slot=0): return self.person(slot)['key']

    def test_complete_emits_the_signer_policy_only(self):
        result = self.validate()
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['status'], 'STRUCTURALLY_COMPLETE')
        self.assertFalse(result['signature_verification_performed'] or result['identity_or_independence_verified'] or result['production_accepted'])
        policy = result['signer_policy']
        # The node's canonical record: compact, its field order, keys sorted.
        decoded = json.loads(policy)
        self.assertEqual(list(decoded), ['schema', 'chain_id', 'authority'])
        self.assertEqual((decoded['schema'], decoded['chain_id'], decoded['authority']['threshold']), (1, CHAIN, 3))
        ids = [k['key_id'] for k in decoded['authority']['keys']]
        self.assertEqual(ids, sorted(ids))
        self.assertEqual(policy, json.dumps(decoded, separators=(',', ':')))
        self.assertFalse(policy.endswith('\n'))
        self.assertEqual(result['signer_policy_sha256'], hashlib.sha256(policy.encode()).hexdigest())
        for key in decoded['authority']['keys']:
            self.assertEqual(key['key_id'], hashlib.sha256(bytes.fromhex(key['public_key_hex'])).hexdigest())

    def test_the_template_is_incomplete(self):
        template = json.loads(TEMPLATE.read_text())
        self.assertEqual(set(template), g.TOP)
        self.assertEqual(g.validate(template, self.root, self.emergency, self.upgrade)['status'], 'INCOMPLETE_OR_INVALID')

    def test_approved_policy_is_fixed(self):
        for key, value in (('threshold', 2), ('authority_size', 4), ('production_accepted', True)):
            self.setUp(); self.data[key] = value
            self.check_bad(key if key != 'production_accepted' else 'production acceptance')
        self.setUp(); self.data['profile']['parameter_set'] = 'SLH-DSA-SHA2-256s'
        self.check_bad('parameter set')
        self.setUp(); self.data['signers'].pop()
        self.check_bad('exactly five genesis signers')

    def test_the_chain_identity_is_required(self):
        for chain in (None, '', 'chain with spaces', 'x'*129):
            self.setUp(); self.data['chain_id'] = chain
            self.check_bad('chain_id')

    def test_keys_are_canonical_and_distinct(self):
        self.key()['public_key_hex'] = bytes([0xab]*g.KEY_BYTES).hex().upper()
        self.check_bad('lowercase hex')
        self.setUp(); self.key()['public_key_hex'] = '00'*g.KEY_BYTES
        self.check_bad('lowercase hex')
        self.setUp(); self.key()['key_id'] = '0'*64
        self.check_bad('SHA-256')
        self.setUp(); self.data['signers'][1]['key'] = dict(self.key(0))
        self.check_bad('reused')

    def test_signers_are_separate_from_the_emergency_and_upgrade_custodians(self):
        emergency_key = base64.b64decode(self.emergency['custodians'][0]['keys']['freeze']['public_key_base64'])
        upgrade_key = base64.b64decode(self.upgrade['custodians'][0]['key']['public_key_base64'])
        for raw, role in ((emergency_key, 'emergency'), (upgrade_key, 'upgrade')):
            self.setUp()
            self.key().update(public_key_hex=raw.hex(), key_id=hashlib.sha256(raw).hexdigest())
            for kind in u.KEY_EVIDENCE:
                self.evidence(self.key()[kind], kind, self.person()['controller_id'], g.PURPOSE, self.key()['key_id'])
            self.check_bad(f'key also holds an {role} role')
        self.setUp(); self.person()['controller_id'] = self.upgrade['custodians'][2]['controller_id']
        self.check_bad('controller is also an upgrade custodian')
        self.setUp(); self.key()['backup_control_group'] = self.emergency['custodians'][1]['control_group']
        self.check_bad('crosses signer groups')
        self.setUp()
        shared = self.upgrade['custodians'][3]['control_group']
        self.person()['control_group'] = shared
        self.key().update(signer_control_group=shared, backup_control_group=shared)
        self.check_bad('control group is shared with an upgrade custodian')

    def test_both_other_intakes_must_be_complete(self):
        self.upgrade = None
        self.check_bad('upgrade intake required')
        self.setUp(); self.emergency = None
        self.check_bad('emergency intake required')
        self.setUp(); self.upgrade['custodians'].pop()
        self.check_bad('upgrade intake incomplete')
        self.setUp(); self.upgrade['custodians'][0]['key']['public_key_base64'] = None
        self.check_bad('upgrade intake incomplete')

    def test_keyed_evidence_names_the_genesis_purpose_without_an_epoch(self):
        ref = self.key()['proof_of_possession']
        self.evidence(ref, 'proof_of_possession', self.person()['controller_id'], 'upgrade', self.key()['key_id'])
        self.check_bad(ref + ': subject, purpose')
        self.setUp()
        ref = self.key()['drill_record']
        self.evidence(ref, 'drill_record', self.person()['controller_id'], g.PURPOSE, self.key()['key_id'], epoch=1)
        self.check_bad(ref + ': subject, purpose')
        self.setUp(); self.evidence('extra', 'appointment', 'nobody')
        self.check_bad('unreferenced evidence')
        self.setUp()
        self.evidence('i0', 'independence_review', self.person()['controller_id'], reviewer=self.person()['control_group'])
        self.check_bad('self-review')

    def test_the_policy_matches_the_offline_signer(self):
        # Real SLH-DSA public key records and what `dytallix-root-sign policy`
        # writes from them (fixtures/genesis-signer-policy); the Go signer's
        # test checks the same bytes.
        fixture = Path(__file__).resolve().parent/'fixtures'/'genesis-signer-policy'
        for i, signer in enumerate(self.data['signers']):
            record = json.loads((fixture/f'signer-{i+1}.json').read_text())
            signer['key'].update(key_id=record['key_id'], public_key_hex=record['public_key_hex'])
            for kind in u.KEY_EVIDENCE:
                self.evidence(signer['key'][kind], kind, signer['controller_id'], g.PURPOSE, record['key_id'])
        result = self.validate()
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['signer_policy'].encode(), (fixture/'policy.json').read_bytes())

    def test_the_cli_writes_the_policy_once(self):
        packet = self.root/'GENESIS_INTAKE.working.json'
        packet.write_text(json.dumps(self.data))
        emergency, upgrade = self.root/'emergency.json', self.root/'upgrade.json'
        emergency.write_text(json.dumps(self.emergency)); upgrade.write_text(json.dumps(self.upgrade))
        policy = self.root/'root-genesis-policy.json'
        command = [sys.executable, str(Path(g.__file__)), str(packet), '--emergency', str(emergency), '--upgrade', str(upgrade), '--policy-out', str(policy)]
        done = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(policy.read_text(), json.loads(done.stdout)['signer_policy'])
        # An existing policy file is never replaced.
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        incomplete = [sys.executable, str(Path(g.__file__)), str(TEMPLATE)]
        self.assertEqual(subprocess.run(incomplete, capture_output=True).returncode, 2)


if __name__ == '__main__': unittest.main(verbosity=2)
