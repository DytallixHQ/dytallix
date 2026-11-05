"""The attack campaign (root-control family) without a chain: the bytes each
attempt sends, the outcome of each answer, and whole campaigns against a
stand-in node that checks a control in the node's order (its canonical
bytes, the freeze for a kit replacement, the sequence, the epoch, the
window, the freeze state, the threshold, the keys, and only then the
signatures), including nodes that admit an attack or check in another
order. The staging network runs it for real in the Host install workflow."""
import hashlib
import json
import unittest

import attacks
import results
import root_controls
from root_controls import encode

CHAIN = 'dytallix-staging-1'
GENESIS, ENGINE = b'{"native": 1}', b'{"engine": 1}'
KIT_KIND = 'dytallix-root-kit-replacement-v1'
KEYS = ['k1', 'k2', 'k3', 'k4', 'k5']
OLD_KEY = 'k5-old'
REFUSED = 'Invalid transaction or observation: '


def sign(key, payload):
    return hashlib.sha256((key + json.dumps(payload, sort_keys=True)).encode()).hexdigest() * 2


def emergency(action, sequence, window, signers=('k1', 'k2', 'k3'), incident='aa' * 32, epoch=2):
    payload = {'schema': 2, 'chain_id': CHAIN, 'release_sha512': 'bb' * 64, 'action': action,
               'sequence': sequence, 'parent_height': window[0] - 1, 'parent_app_hash': 'cc' * 32,
               'target_height': window[0],
               'v2': {'genesis_sha256': 'dd' * 32, 'authority_epoch': epoch, 'policy_sha256': 'ee' * 32,
                      'not_before_height': window[0], 'not_after_height': window[1], 'incident_sha256': incident}}
    return encode({'kind': root_controls.EMERGENCY_KIND, 'payload': payload,
                   'signatures': [{'key_id': k, 'signature_hex': sign(k, payload)} for k in sorted(signers)]})


def replacement(window=(110, 34_669)):
    payload = {'schema': 1, 'chain_id': CHAIN, 'authority_epoch': 2, 'sequence': 3, 'anchor_height': 109,
               'not_before_height': window[0], 'not_after_height': window[1], 'replaced': {'upgrade': 'k4'}}
    return encode({'kind': KIT_KIND, 'payload': payload,
                   'signatures': [{'key_id': k, 'signature_base64': sign(k, payload)} for k in ('k1', 'k2', 'k3')],
                   'proofs': {'upgrade': sign('new', payload)}})


class FakeNode:
    """CheckTx for root controls, in the node's order; `submit` applies an
    admitted freeze or resume."""

    def __init__(self, threshold=3, canonical=True, signatures_first=False):
        self.threshold, self.canonical, self.signatures_first = threshold, canonical, signatures_first
        self.next_sequence, self.frozen, self.height = 7, False, 120

    def signed(self, value):
        payload = value['payload']
        field = 'signature_hex' if value['kind'] == root_controls.EMERGENCY_KIND else 'signature_base64'
        return all(s[field] == sign(s['key_id'], payload) for s in value['signatures'])

    def answer(self, raw, submit):
        value = json.loads(raw)
        # The node decodes into its structs and writes them again, in their order.
        model = json.loads(replacement() if value['kind'] == KIT_KIND else emergency('freeze', 0, (1, 2)))
        if self.canonical and encode(root_controls.in_order_of(model, value)) != raw:
            return 1, REFUSED + f'unknown variant `{value["kind"]}`, expected one of `observation`'
        if self.signatures_first and not self.signed(value):
            return 1, REFUSED + 'Emergency signature verification'
        if value['kind'] == KIT_KIND:
            if self.frozen:
                return 1, REFUSED + 'Emergency transaction freeze'
            return (0, '') if self.signed(value) else (1, REFUSED + 'Kit replacement signature')
        payload = value['payload']
        v2 = payload['v2']
        refusal = next((reason for wrong, reason in (
            (payload['sequence'] != self.next_sequence, 'Emergency sequence mismatch/exhaustion'),
            (v2['authority_epoch'] != 2, 'Emergency v2 policy/genesis/epoch binding'),
            (not v2['not_before_height'] <= self.height + 1 <= v2['not_after_height'],
             'Emergency v2 outside validity window'),
            (self.frozen == (payload['action'] == 'freeze'), 'Emergency already frozen or not frozen'),
            (len(value['signatures']) < self.threshold, 'Emergency signature threshold'),
            ([s['key_id'] for s in value['signatures']] != sorted(s['key_id'] for s in value['signatures']),
             'Emergency signatures must be strictly sorted'),
            (any(s['key_id'] not in KEYS for s in value['signatures']), 'Emergency authority key missing'),
            (not self.signed(value), 'Emergency signature verification'),
        ) if wrong), None)
        if refusal:
            return 1, REFUSED + refusal
        if submit:
            self.frozen = payload['action'] == 'freeze'
            self.next_sequence += 1
        return 0, ''


class FakeDesk:
    """The genuine controls, made from the stand-in node's keys, and the node
    through a CLI that can fail to answer."""

    def __init__(self, node, unavailable=0):
        self.node, self.unavailable = node, unavailable
        self.template = emergency('freeze', node.next_sequence, (100, 34_659))
        self.expired = emergency('freeze', node.next_sequence, (90, 95))
        self.old_signature = {'key_id': OLD_KEY,
                              'signature_hex': sign(OLD_KEY, json.loads(self.template)['payload'])}
        self.consumed = {'freeze': emergency('freeze', 5, (40, 34_599)),
                         'resume': emergency('resume', 6, (60, 34_619))}
        self.replacement = replacement()
        self.resume_control = None
        self.waited = 0

    def answer(self, raw, submit):
        if self.unavailable:
            self.unavailable -= 1
            raise root_controls.Unavailable('the CLI got no answer')
        code, log = self.node.answer(raw, submit)
        return {'code': code, 'log': log}

    def check(self, raw): return self.answer(raw, False)

    def submit(self, raw): return self.answer(raw, True)

    def height(self):
        self.node.height += 1
        return self.node.height

    def wait_expired(self): self.waited += 1

    def freeze(self):
        if self.submit(self.template)['code']:
            raise root_controls.Failed('the freeze was refused')
        self.resume_control = emergency('resume', self.node.next_sequence, (self.node.height, 34_700))

    def resume(self):
        if self.submit(self.resume_control)['code']:
            raise root_controls.Failed('the resume was refused')


def campaign(node=None, attempts=4, kinds=tuple(root_controls.REASONS), desk=None):
    desk = desk or FakeDesk(node or FakeNode())
    rows = root_controls.Campaign(desk, attempts, show=lambda _: None, sleep=lambda _: None).run(list(kinds))
    record = root_controls.build_record(CHAIN, GENESIS, ENGINE, rows, attempts, root_controls.materials_of(desk),
                                        '2026-11-05T00:00:00Z', '2026-11-05T00:10:00Z')
    return desk, rows, record


class Bytes(unittest.TestCase):
    def test_each_attempt_changes_exactly_its_types_fields(self):
        desk = FakeDesk(FakeNode())
        for kind in root_controls.REASONS:
            sent = []
            for k in range(9):
                variant, route, raw, genuine = root_controls.forge(kind, k, desk)
                row = {'type': kind, 'variant': variant, **root_controls.facts(raw, genuine)}
                self.assertTrue(root_controls.as_named(row), (kind, k, row['changed'], row['canonical']))
                self.assertEqual(route, 'check' if variant in ('freeze', 'resume') else 'submit')
                sent.append(raw)
            # Every attempt sent has its own bytes; committed ones are checked again.
            submitted = [raw for k, raw in enumerate(sent)
                         if root_controls.variant_of(kind, k) not in ('freeze', 'resume')]
            self.assertEqual(len(set(submitted)), len(submitted), kind)
        first = root_controls.forge('too_few_signatures', 0, desk)[2]
        self.assertEqual(len(json.loads(first)['signatures']), 2)
        swapped = json.loads(root_controls.forge('wrong_epoch_keys', 0, desk)[2])['signatures']
        self.assertEqual([s['key_id'] for s in swapped], ['k2', 'k3', OLD_KEY])

    def test_reencoded_bytes_keep_the_genuine_value_and_signatures(self):
        genuine = FakeDesk(FakeNode()).template
        for k in range(6):
            variant, raw = root_controls.reencoded(genuine, k)
            self.assertEqual(json.loads(raw), json.loads(genuine), variant)
            self.assertFalse(root_controls.is_canonical(raw, genuine), variant)
            self.assertTrue(raw.endswith(b' ' * (k // 3)))
        self.assertTrue(root_controls.is_canonical(genuine, genuine))
        self.assertTrue(root_controls.reencoded(genuine, 1)[1].startswith(b'{"\\u006bind":'))
        self.assertTrue(root_controls.reencoded(genuine, 2)[1].startswith(b'{"payload":'))

    def test_an_answer_counts_only_for_its_reason_and_its_own_fault(self):
        row = {'type': 'too_few_signatures', 'variant': 'altered', 'changed': ['payload.v2.incident_sha256',
                                                                               'signatures'],
               'canonical': True, 'code': 1, 'log': REFUSED + 'Emergency signature threshold'}
        self.assertEqual(root_controls.classify(row), 'refused')
        self.assertEqual(root_controls.classify({**row, 'log': REFUSED + 'Emergency signature verification'}),
                         'refused_other')
        self.assertEqual(root_controls.classify({**row, 'changed': ['signatures']}), 'refused_other')
        self.assertEqual(root_controls.classify({**row, 'canonical': False}), 'refused_other')
        self.assertEqual(root_controls.classify({**row, 'code': 0}), 'admitted')
        no_answer = {k: v for k, v in row.items() if k not in ('code', 'log')}
        self.assertEqual(root_controls.classify({**no_answer, 'error': 'no answer'}), 'transport')
        with self.assertRaises(root_controls.Failed):
            root_controls.classify({**row, 'type': 'freeze_without_keys'})


class Campaigns(unittest.TestCase):
    def test_a_node_refuses_every_type_for_its_reason_and_the_chain_resumes(self):
        desk, rows, record = campaign()
        self.assertEqual(record['status'], 'PASS', record['checks'])
        for kind, found in record['types'].items():
            self.assertEqual((found['status'], found['counts']['refused']), ('PASS', 4), kind)
            self.assertIn(root_controls.REASONS[kind], found['first_log'])
        self.assertEqual([r['what'] for r in rows if r['type'] == 'control'],
                         ['template'] * 6 + ['replacement', 'resume_control', 'replacement'])
        self.assertEqual(record['controls'], {'made': 9, 'admitted': 9})
        self.assertFalse(desk.node.frozen)
        self.assertEqual(desk.waited, 1)
        self.assertEqual(record['materials']['expired_window'], [90, 95])
        raw = attacks.rows_bytes(rows)
        self.assertEqual(root_controls.verify(record, raw, GENESIS, ENGINE), 'PASS')

    def test_a_node_that_admits_an_attack_fails_and_its_chain_is_not_frozen_again(self):
        _, _, record = campaign(FakeNode(threshold=2))
        self.assertEqual(record['types']['too_few_signatures']['status'], 'FAIL')
        self.assertGreater(record['successes'], 0)
        self.assertEqual(record['status'], 'FAIL')
        # The admitted freeze held the chain; the freeze type is not run on it.
        self.assertEqual(record['types']['kit_replacement_while_frozen']['status'], 'not_run')
        _, _, record = campaign(FakeNode(canonical=False), kinds=['non_canonical'])
        self.assertEqual((record['types']['non_canonical']['status'], record['successes']), ('FAIL', 1))

    def test_a_node_that_verifies_signatures_first_does_not_test_the_named_check(self):
        _, rows, record = campaign(FakeNode(signatures_first=True), kinds=['too_few_signatures'])
        first, later = rows[1], rows[2]
        self.assertEqual((first['variant'], first['outcome']), ('signed', 'refused'))
        self.assertEqual((later['variant'], later['outcome']), ('altered', 'refused_other'))
        self.assertEqual(record['types']['too_few_signatures']['status'], 'FAIL')

    def test_an_attempt_the_cli_could_not_make_is_made_again(self):
        desk = FakeDesk(FakeNode())
        run = root_controls.Campaign(desk, 2, show=lambda _: None, sleep=lambda _: None)
        desk.unavailable = 1
        run.run_type('expired_window')
        self.assertEqual([r['outcome'] for r in run.rows], ['transport', 'refused', 'refused'])
        self.assertEqual(root_controls.tally(run.rows, 2)['expired_window']['status'], 'PASS')
        desk.unavailable = root_controls.TRIES
        with self.assertRaisesRegex(root_controls.Failed, 'could not be made'):
            run.attempt('expired_window', 0)

    def test_a_chain_that_stops_fails_its_control(self):
        desk = FakeDesk(FakeNode())
        desk.height = lambda: 130
        ticks = iter(range(0, 1000, 10))
        stalled = root_controls.Campaign(desk, 1, show=lambda _: None, clock=lambda: next(ticks),
                                         sleep=lambda _: None)
        with self.assertRaisesRegex(root_controls.Failed, 'did not advance past height 130'):
            stalled.run(['expired_window'])

    def test_a_control_the_node_refuses_fails_the_record(self):
        desk = FakeDesk(FakeNode())
        desk.template = emergency('freeze', 3, (100, 34_659))
        desk.consumed = {'freeze': emergency('freeze', 1, (40, 34_599)), 'resume': emergency('resume', 2, (60, 99))}
        _, rows, record = campaign(desk=desk, kinds=['replayed_sequence'])
        self.assertFalse(record['checks']['controls'])
        self.assertEqual(record['status'], 'FAIL')

    def test_no_attempt_is_made_from_a_control_this_campaign_would_encode_otherwise(self):
        node = FakeNode()
        desk = FakeDesk(node)
        desk.consumed['resume'] = json.dumps(json.loads(desk.consumed['resume']), indent=1).encode()
        with self.assertRaisesRegex(root_controls.Failed, 'committed resume control is not in the encoding'):
            campaign(desk=desk)
        self.assertEqual(node.height, 120)

    def test_verify_recomputes_every_count_from_the_rows(self):
        _, rows, record = campaign(attempts=2)
        raw = attacks.rows_bytes(rows)
        edited = dict(rows[1], outcome='refused_other')
        with self.assertRaisesRegex(root_controls.Failed, 'rows are not the ones'):
            root_controls.verify(record, attacks.rows_bytes([rows[0], edited] + rows[2:]), GENESIS, ENGINE)
        with self.assertRaisesRegex(root_controls.Failed, 'another genesis'):
            root_controls.verify(record, raw, b'{}', ENGINE)
        tampered = dict(record, attempts_wanted=1)
        with self.assertRaisesRegex(root_controls.Failed, 'digest does not match'):
            root_controls.verify(tampered, raw, GENESIS, ENGINE)
        body = {k: v for k, v in record.items() if k != 'sha256'}
        body['rows_sha256'] = attacks.digest(attacks.rows_bytes([rows[0], edited] + rows[2:]))
        forged = dict(body, sha256=root_controls.record_digest(body))
        with self.assertRaisesRegex(root_controls.Failed, 'does not follow from its answer'):
            root_controls.verify(forged, attacks.rows_bytes([rows[0], edited] + rows[2:]), GENESIS, ENGINE)

    def test_the_launch_page_counts_the_familys_fewest_type(self):
        _, rows, record = campaign(attempts=3)
        attempts, successes = results.attack_kpis('root_controls', [record, record])
        self.assertEqual((attempts['id'], attempts['value'], attempts['status'], attempts['target']),
                         ('campaign.root_controls.attempts', 6, 'in_progress', 3000))
        self.assertEqual((successes['value'], successes['status']), (0, 'in_progress'))
        raw = attacks.rows_bytes(rows)
        families, sources, _ = results.attack_records([(json.dumps(record).encode(), raw)], GENESIS, ENGINE)
        self.assertEqual((list(families), sources[0]['tool'], sources[0]['status']),
                         (['root_controls'], 'attacks.root_controls', 'PASS'))


if __name__ == '__main__':
    unittest.main()
