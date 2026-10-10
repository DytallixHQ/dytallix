"""Synthetic five-holder custody packets exercise the five_holders model and
the combined check. No real key, person or approval."""
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import emergency_custodian_intake as e
import five_holder_custody as f
import genesis_signer_intake as g
import upgrade_custodian_intake as u

STATEMENT = 'Synthetic structural test only. No real key, signature, appointment or approval.'
CHAIN = 'dytallix-staging-1'
EPOCH = 1


class Packets:
    """The three packets for five holders, each in its own directory with its evidence."""

    def __init__(self, base, reviewed=False):
        self.base, self.reviewed, self.seed = base, reviewed, 0
        self.packets = {'emergency': self.packet('emergency'), 'upgrade': self.packet('upgrade'),
                        'genesis': self.packet('genesis')}

    def root(self, name):
        path = self.base / name
        path.mkdir(exist_ok=True)
        return path

    def key(self):
        self.seed += 1
        return bytes([self.seed]) * u.KEY_BYTES

    def evidence(self, packet, name, ref, kind, controller=None, purpose=None, key=None, epoch=None):
        item = {'kind': kind, 'controller_id': controller, 'purpose': purpose, 'key_id': key,
                'parameter_set': u.PARAMETER_SET, 'epoch': epoch,
                'reviewer_control_group': 'synthetic-outside-reviewer' if kind == 'independence_review' else None,
                'public_statement': STATEMENT}
        raw = json.dumps(item).encode()
        (self.root(name) / (ref + '.json')).write_bytes(raw)
        packet['evidence'][ref] = {'path': ref + '.json', 'sha256': hashlib.sha256(raw).hexdigest()}
        return ref

    def key_record(self, packet, name, slot, purpose, epoch):
        controller, group = u.kit_names(slot)
        raw = self.key()
        key_id = hashlib.sha256(raw).hexdigest()
        record = {'key_id': key_id, 'signer_control_group': group, 'backup_control_group': group}
        if name == 'genesis':
            record['public_key_hex'] = raw.hex()
        else:
            record['public_key_base64'] = base64.b64encode(raw).decode()
        for kind in u.KEY_EVIDENCE:
            record[kind] = self.evidence(packet, name, f'{slot}-{purpose}-{kind}', kind, controller, purpose, key_id, epoch)
        return record

    def packet(self, name):
        top = {'emergency': {'schema': e.SCHEMA, 'freeze_threshold': 3, 'resume_threshold': 3, 'authority_epoch': EPOCH},
               'upgrade': {'schema': u.SCHEMA, 'threshold': 3, 'authority_epoch': EPOCH},
               'genesis': {'schema': g.SCHEMA, 'threshold': 3, 'chain_id': CHAIN}}[name]
        packet = {**top, 'production_accepted': False, 'custody_model': u.FIVE, 'authority_size': 5,
                  'profile': {'parameter_set': u.PARAMETER_SET, 'approval': 'profile'}, 'evidence': {}}
        self.evidence(packet, name, 'profile', 'profile_approval')
        people = []
        for slot in range(1, 6):
            controller, group = u.kit_names(slot)
            person = {'slot': slot, 'controller_id': controller, 'name': f'Synthetic holder {slot}',
                      'organization': 'Synthetic', 'control_group': group,
                      'appointment': self.evidence(packet, name, f'appointment-{slot}', 'appointment', controller),
                      'independence_review': (self.evidence(packet, name, f'review-{slot}', 'independence_review', controller)
                                              if self.reviewed else None)}
            if name == 'emergency':
                person['keys'] = {p: self.key_record(packet, name, slot, p, EPOCH) for p in e.PURPOSES}
            elif name == 'upgrade':
                person['key'] = self.key_record(packet, name, slot, u.PURPOSE, EPOCH)
            else:
                person['key'] = self.key_record(packet, name, slot, g.PURPOSE, None)
            people.append(person)
        packet['signers' if name == 'genesis' else 'custodians'] = people
        return packet

    def roots(self): return {name: self.root(name) for name in self.packets}

    def check(self): return f.check(self.packets, self.roots())


class FiveHolderCustodyTests(unittest.TestCase):
    def setUp(self, reviewed=False):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.set = Packets(Path(self.tmp.name).resolve(), reviewed)
        self.packets = self.set.packets

    def bad(self, message):
        result = self.set.check()
        self.assertEqual(result['status'], 'INCOMPLETE_OR_INVALID')
        self.assertNotIn('signer_policy', result)
        self.assertTrue(any(message in error for error in result['errors']), result['errors'])

    def test_five_holders_with_the_public_disclosure(self):
        result = self.set.check()
        self.assertEqual(result['errors'], [])
        self.assertEqual((result['status'], result['custody_review']), ('STRUCTURALLY_COMPLETE', 'public_disclosure'))
        self.assertIn('OPEN', result['d10_q03'])
        self.assertFalse(result['signature_verification_performed'] or result['production_accepted'])
        self.assertEqual(len(result['emergency_authority_fragment']['freeze']['keys']), 5)
        self.assertEqual(len(result['upgrade_authority_fragment']['authority']['keys']), 5)
        self.assertEqual(json.loads(result['signer_policy'])['chain_id'], CHAIN)
        # Each checker alone agrees and names the form.
        self.assertEqual(e.validate(self.packets['emergency'], self.set.root('emergency'))['custody_review'], 'public_disclosure')

    def test_five_holders_with_an_outside_reviewer(self):
        self.setUp(reviewed=True)
        result = self.set.check()
        self.assertEqual(result['errors'], [])
        self.assertEqual(result['custody_review'], 'outside_reviewer')

    def test_every_slot_has_the_same_review_form(self):
        self.setUp(reviewed=True)
        upgrade = self.packets['upgrade']
        upgrade['evidence'].pop('review-3')
        upgrade['custodians'][2]['independence_review'] = None
        self.bad('every slot has an outside review, or none')

    def test_the_packets_use_the_same_review_form(self):
        (Path(self.tmp.name) / 'reviewed').mkdir()
        reviewed = Packets(Path(self.tmp.name).resolve() / 'reviewed', True)
        self.set.packets['genesis'] = reviewed.packets['genesis']
        roots = self.set.roots()
        roots['genesis'] = reviewed.root('genesis')
        result = f.check(self.set.packets, roots)
        self.assertTrue(any('same review form' in error for error in result['errors']), result['errors'])

    def test_holders_are_named_only_by_their_kits(self):
        self.packets['emergency']['custodians'][1]['controller_id'] = 'Alice Example'
        self.bad('names the holder only by the kit')
        self.setUp()
        self.packets['genesis']['signers'][4]['control_group'] = 'kit-1'
        self.bad('controller kit-holder-5, control group kit-5')

    def test_the_same_holders_hold_every_role(self):
        # Two kits swapped in the upgrade packet: each packet alone is
        # named by kit, but slot 1 and 2 hold other kits' roles.
        people = self.packets['upgrade']['custodians']
        people[0]['slot'], people[1]['slot'] = 2, 1
        self.bad('names the holder only by the kit')

    def test_every_packet_is_five_holders(self):
        self.packets['upgrade']['custody_model'] = 'independent'
        self.bad('upgrade: custody_model must be five_holders')

    def test_keys_are_distinct_across_roles(self):
        reused = self.packets['emergency']['custodians'][0]['keys']['freeze']
        raw = base64.b64decode(reused['public_key_base64'])
        key = self.packets['genesis']['signers'][0]['key']
        key['public_key_hex'] = raw.hex()
        old = key['key_id']
        key['key_id'] = reused['key_id']
        # Rebind the genesis key's evidence to the reused key.
        for kind in u.KEY_EVIDENCE:
            ref = key[kind]
            path = self.set.root('genesis') / (ref + '.json')
            item = json.loads(path.read_bytes())
            item['key_id'] = reused['key_id']
            path.write_bytes(json.dumps(item).encode())
            self.packets['genesis']['evidence'][ref]['sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
        self.assertNotEqual(old, reused['key_id'])
        self.bad('key also holds an emergency role')

    def test_one_epoch_for_the_emergency_and_upgrade_keys(self):
        self.packets['upgrade']['authority_epoch'] = 2
        self.bad('share one authority epoch')

    def test_the_command_writes_the_policy_once(self):
        paths = {}
        for name, packet in self.packets.items():
            paths[name] = self.set.root(name) / 'packet.json'
            paths[name].write_text(json.dumps(packet))
        policy = Path(self.tmp.name) / 'policy.json'
        script = Path(f.__file__)
        args = [sys.executable, '-B', str(script), *[x for name in paths for x in ('--' + name, str(paths[name]))],
                '--policy-out', str(policy)]
        run = subprocess.run(args, capture_output=True, text=True, cwd=script.parent)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        out = json.loads(run.stdout)
        self.assertEqual(policy.read_text(), out['signer_policy'])
        self.assertEqual(out['packet_sha256']['upgrade'], hashlib.sha256(paths['upgrade'].read_bytes()).hexdigest())
        # Never replaces the policy file.
        self.assertNotEqual(subprocess.run(args, capture_output=True, cwd=script.parent).returncode, 0)


class ExistingModelsTests(unittest.TestCase):
    def test_independent_still_refuses_shared_holders(self):
        # The five-holder packets relabeled independent: shared kits across
        # roles are what independent forbids.
        with tempfile.TemporaryDirectory() as tmp:
            packets = Packets(Path(tmp).resolve(), reviewed=True)
            for packet in packets.packets.values():
                packet['custody_model'] = 'independent'
            result = u.validate(packets.packets['upgrade'], packets.root('upgrade'), packets.packets['emergency'])
            self.assertTrue(any('controller is also an emergency custodian' in error for error in result['errors']),
                            result['errors'])


if __name__ == '__main__':
    unittest.main()
