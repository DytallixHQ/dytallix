"""The attack campaign (recovery family) without a chain: the outcome of
each answer, and whole campaigns against a stand-in node that checks a
recovery at admission in the node's order (expiry, the consumed
sponsorship or operation, then the guardian quorum), including nodes that
admit a replay or a non-guardian signer, change what a refusal must not, or
give no answer. The desk's reading of the CLI's reports is checked against
a stand-in CLI. The staging network runs it for real in the Host install
workflow."""
import base64
import hashlib
import json
from pathlib import Path
import stat
import sys
import tempfile
import threading
import unittest

import account_recovery as recovery
from account_recovery import REPLAYED, WRONG_KEY
import attacks
import results

CHAIN = 'dytallix-staging-1'
GENESIS, ENGINE = b'{"native": 1}', b'{"engine": 1}'
CONSUMED = 'Consumed sponsorship or successful operation'
NOT_GUARDIAN = 'signer is not a current guardian'
LIFETIME = 17_280


def h(*parts):
    return hashlib.sha256(' '.join(map(str, parts)).encode()).hexdigest()


class FakeNode:
    """Recovery accounts and sponsor receipts, checked at admission as the
    node checks them; each knob a defect."""

    def __init__(self, admit_replays=False, admit_outsiders=False, log=None, unavailable=0, touch=False,
                 fail_enroll=False, lifetime=LIFETIME):
        self.admit_replays, self.admit_outsiders, self.log = admit_replays, admit_outsiders, log
        self.unavailable, self.touch, self.fail_enroll, self.lifetime = unavailable, touch, fail_enroll, lifetime
        self.height = 100
        self.accounts, self.receipts, self.operations = {}, set(), set()
        self.sponsor = {'sponsor_nonce': 0, 'sponsor_liquid_udrt': str(10 ** 12)}
        self.lock = threading.Lock()

    def tick(self):
        with self.lock:
            self.height += 1
            return self.height

    def account(self, label):
        return self.accounts.setdefault(label, {'policy': None, 'active': h('active', label), 'version': 0,
                                                'sequence': 0, 'pending': None, 'changes': 0})

    def answer(self, kind, fault, label):
        with self.lock:
            if self.unavailable:
                self.unavailable -= 1
                raise recovery.Unavailable('the CLI got no answer')
            if self.touch:
                self.account(label)['changes'] += 1
            if kind == REPLAYED:
                consumed = fault['authorization_id'] in self.receipts or fault['operation_id'] in self.operations
                if consumed and not self.admit_replays:
                    return {'code': 1, 'log': self.log or CONSUMED}
                return {'code': 0, 'log': ''}
            account = self.account(label)
            if any(s not in account['policy'] for s in fault['signers']) and not self.admit_outsiders:
                return {'code': 1, 'log': self.log or NOT_GUARDIAN}
            account['pending'] = fault['operation_id']
            return {'code': 0, 'log': ''}


class FakeDesk:
    """The desk's attempts and reads over the stand-in node, its faults made
    as the real desk makes them."""

    def __init__(self, node, targets=2, kinds=2):
        self.node = node
        self.roles = {'targets': [f'load-{n:04}' for n in range(1, targets + 1)],
                      'controls': [f'load-{n:04}' for n in range(targets + 1, targets + kinds + 1)],
                      'sponsor': f'load-{targets + kinds + 1:04}'}
        self.committed, self.enrolled = {}, {}
        self.bounds, self.lifetime = (200_000, 2_000_000), node.lifetime
        self.made = 0

    def head(self):
        return self.node.tick()

    def state(self, label):
        a = self.node.account(label)
        return {'account': recovery.record_digest(a), **self.node.sponsor}

    def states(self, labels):
        return {w: self.state(w) for w in labels}

    def ready(self, kind, labels):
        return self.head(), self.states(labels), {}

    def enroll(self, label):
        node = self.node
        operation, authorization = h('enroll', label), h('enroll sponsor', label, node.sponsor['sponsor_nonce'])
        account = node.account(label)
        expiry = node.height + node.lifetime
        receipt = None
        if not node.fail_enroll:
            account.update(policy=[h('guardian', label, n) for n in (1, 2, 3)], version=1)
            node.receipts.add(authorization)
            node.operations.add(operation)
            node.sponsor['sponsor_nonce'] += 1
            receipt = {'success': True, 'block_height': node.tick(), 'retained_until': expiry, 'gas_used': 180_000,
                       'settled_fee': '1800000', 'operation_id': operation, 'authorization_id': authorization}
        self.enrolled[label] = {'operation_id': operation, 'authorization_id': authorization, 'expiry': expiry,
                                'envelope_sha256': h('envelope', label), 'engine_hash': h('wire', label),
                                'sponsor_nonce': 0, 'receipt': receipt}
        return {'code': 0, 'log': '', 'operation_id': operation, 'authorization_id': authorization,
                'receipt': receipt, 'enrolled': receipt is not None}

    def keep_committed(self):
        for label in self.roles['targets']:
            e = self.enrolled[label]
            self.committed[label] = {**e, 'success': True, 'block_height': e['receipt']['block_height'],
                                     'retained_until': e['receipt']['retained_until']}

    def attempt(self, kind, k, label, views):
        self.made += 1
        if kind == REPLAYED:
            c = self.committed[label]
            variant = recovery.REPLAYS[k % 2]
            kept = {key: c[key] for key in ('operation_id', 'authorization_id', 'envelope_sha256', 'engine_hash',
                                            'success', 'block_height', 'retained_until')}
            if variant == 'committed':
                route = 'check'
                fault = {'operation_id': c['operation_id'], 'authorization_id': c['authorization_id'],
                         'envelope_sha256': c['envelope_sha256'], 'tx_sha256': c['engine_hash'],
                         'sponsor_nonce_signed': 0, 'sponsor_expiry': c['expiry'], 'committed': kept}
            else:
                route = 'submit'
                fault = {'operation_id': c['operation_id'], 'authorization_id': h('again', label, k),
                         'envelope_sha256': h('bytes', self.made),
                         'sponsor_nonce_signed': self.node.sponsor['sponsor_nonce'],
                         'sponsor_expiry': self.node.height + 20 + k, 'committed': kept}
        else:
            variant, route = list(recovery.SHAPES)[k % 3], 'submit'
            account = self.node.account(label)
            g, active = account['policy'], account['active']
            signers = {'outsiders': [h('outsider', 1), h('outsider', 2)], 'with_guardian': [g[0], h('outsider', 1)],
                       'active_key': [g[0], active]}[variant]
            fault = {'operation_id': h('start', label, k), 'signers': signers, 'guardians': list(g), 'active': active,
                     'replacement': h('replacement'),
                     'authority': {'status': 'Normal', 'pending': account['pending'] is not None,
                                   'policy_version': str(account['version']), 'recovery_sequence': '0'}}
        found = {'variant': variant, 'route': route, 'fault': fault}
        try:
            found.update(self.node.answer(kind, fault, label))
        except recovery.Unavailable as error:
            found['error'] = str(error)
        return found


def campaign(node=None, attempts=4, kinds=tuple(recovery.REASONS), targets=2):
    desk = FakeDesk(node or FakeNode(), targets=targets, kinds=len(kinds))
    rows = recovery.Campaign(desk, attempts, show=lambda _: None, sleep=lambda _: None).run(list(kinds))
    record = recovery.build_record(CHAIN, GENESIS, ENGINE, rows, attempts, materials(desk), '2026-11-06T00:00:00Z',
                                   '2026-11-06T01:00:00Z')
    return desk, rows, record


def materials(desk):
    return {'targets': len(desk.roles['targets']), 'gas_limit': desk.bounds[0],
            'maximum_charge_udrt': str(desk.bounds[1]), 'submission_lifetime': desk.lifetime,
            'committed': [{k: c[k] for k in ('operation_id', 'authorization_id', 'block_height', 'retained_until')}
                          for c in desk.committed.values()], 'replays': list(recovery.REPLAYS),
            'shapes': list(recovery.SHAPES)}


def replay_row(variant='committed', **changes):
    committed = {'operation_id': 'op', 'authorization_id': 'auth', 'envelope_sha256': 'committed',
                 'engine_hash': 'wire', 'success': True, 'block_height': 90, 'retained_until': 1_000}
    if variant == 'committed':
        fault = {'operation_id': 'op', 'authorization_id': 'auth', 'envelope_sha256': 'committed',
                 'tx_sha256': 'wire', 'sponsor_nonce_signed': 0, 'sponsor_expiry': 1_000, 'committed': committed}
    else:
        fault = {'operation_id': 'op', 'authorization_id': 'auth-2', 'envelope_sha256': 'new',
                 'sponsor_nonce_signed': 3, 'sponsor_expiry': 1_000, 'committed': committed}
    state = {'account': 'a', 'sponsor_nonce': 3, 'sponsor_liquid_udrt': '100'}
    row = {'type': REPLAYED, 'k': 0, 'variant': variant, 'route': 'check' if variant == 'committed' else 'submit',
           'target': 0, 'height': 120, 'fault': fault, 'code': 1, 'log': CONSUMED, 'before': dict(state),
           'after': dict(state)}
    for key, value in changes.items():
        if key in fault:
            fault[key] = value
        elif key in committed:
            committed[key] = value
        else:
            row[key] = value
    return row


def start_row(variant='outsiders', signers=None, **changes):
    guardians, active = ['g1', 'g2', 'g3'], 'active'
    signers = signers or {'outsiders': ['o1', 'o2'], 'with_guardian': ['g1', 'o1'], 'active_key': ['g1', active]}[variant]
    fault = {'operation_id': 'start', 'signers': signers, 'guardians': guardians, 'active': active,
             'replacement': 'r', 'authority': {'status': 'Normal', 'pending': False, 'policy_version': '1',
                                               'recovery_sequence': '0'}}
    fault.update(changes)
    state = {'account': 'a', 'sponsor_nonce': 3, 'sponsor_liquid_udrt': '100'}
    return {'type': WRONG_KEY, 'k': 0, 'variant': variant, 'route': 'submit', 'target': 0, 'height': 120,
            'fault': fault,
            'code': 1, 'log': NOT_GUARDIAN, 'before': dict(state), 'after': dict(state)}


class Outcomes(unittest.TestCase):
    def test_a_replay_counts_only_as_the_committed_operation_before_its_expiry(self):
        for variant in recovery.REPLAYS:
            self.assertEqual(recovery.classify(replay_row(variant)), 'refused', variant)
        self.assertEqual(recovery.classify(replay_row(operation_id='other')), 'refused_other')
        self.assertEqual(recovery.classify(replay_row(success=False)), 'refused_other')
        # The next block is the operation's expiry: the node answers expired first.
        self.assertEqual(recovery.classify(replay_row(height=999)), 'refused_other')
        self.assertEqual(recovery.classify(replay_row('responsored', sponsor_expiry=121)), 'refused_other')
        # The committed bytes are the ones the node hashed, asked at admission directly.
        self.assertEqual(recovery.classify(replay_row(tx_sha256='other')), 'refused_other')
        self.assertEqual(recovery.classify(replay_row(route='submit')), 'refused_other')
        self.assertEqual(recovery.classify(replay_row(authorization_id='auth-3')), 'refused_other')
        # A new sponsorship: a new authorization at the sponsor's current nonce, in new bytes.
        self.assertEqual(recovery.classify(replay_row('responsored', authorization_id='auth')), 'refused_other')
        self.assertEqual(recovery.classify(replay_row('responsored', sponsor_nonce_signed=2)), 'refused_other')
        self.assertEqual(recovery.classify(replay_row('responsored', envelope_sha256='committed')), 'refused_other')

    def test_a_wrong_key_counts_only_in_its_shape_with_the_chains_guardians(self):
        for variant in recovery.SHAPES:
            self.assertEqual(recovery.classify(start_row(variant)), 'refused', variant)
        self.assertEqual(recovery.classify(start_row('outsiders', ['g1', 'o1'])), 'refused_other')
        self.assertEqual(recovery.classify(start_row('with_guardian', ['g1', 'g2'])), 'refused_other')
        self.assertEqual(recovery.classify(start_row('active_key', ['o1', 'active'])), 'refused_other')
        self.assertEqual(recovery.classify(start_row('outsiders', ['o1', 'o1'])), 'refused_other')
        self.assertEqual(recovery.classify(start_row(replacement='g2')), 'refused_other')
        self.assertEqual(recovery.classify(start_row(guardians=['g1', 'g2'])), 'refused_other')
        self.assertEqual(recovery.classify(start_row(authority={'status': 'PendingRecovery', 'pending': True})),
                         'refused_other')

    def test_an_answer_admitted_or_a_change_is_a_success_and_no_answer_is_transport(self):
        self.assertEqual(recovery.classify(replay_row(code=0)), 'admitted')
        row = start_row()
        row['after'] = dict(row['after'], sponsor_nonce=4)
        self.assertEqual(recovery.classify(row), 'admitted')
        self.assertEqual(recovery.classify(replay_row(log='Expired sponsorship or operation')), 'refused_other')
        row = replay_row()
        del row['code']
        self.assertEqual(recovery.classify(row), 'transport')
        with self.assertRaises(recovery.Failed):
            recovery.classify(dict(start_row(), type='rotate'))

    def test_a_control_needs_its_verified_receipt_a_success_and_the_policy_enrolled(self):
        receipt = {'success': True, 'operation_id': 'op', 'authorization_id': 'auth'}
        row = {'type': 'control', 'what': 'enroll', 'height': 5, 'code': 0, 'operation_id': 'op',
               'authorization_id': 'auth', 'receipt': receipt, 'enrolled': True}
        self.assertTrue(recovery.control_ok(row))
        self.assertFalse(recovery.control_ok(dict(row, receipt=dict(receipt, success=False))))
        self.assertFalse(recovery.control_ok(dict(row, receipt=None)))
        self.assertFalse(recovery.control_ok(dict(row, enrolled=False)))
        self.assertFalse(recovery.control_ok(dict(row, receipt=dict(receipt, authorization_id='other'))))


class Campaigns(unittest.TestCase):
    def test_an_honest_node_refuses_every_attempt_and_the_controls_commit(self):
        desk, rows, record = campaign(attempts=6)
        self.assertEqual(record['status'], recovery.PASS, record['checks'])
        for kind in recovery.REASONS:
            self.assertEqual(record['types'][kind]['counts']['refused'], 6)
            self.assertEqual(record['types'][kind]['first_log'], recovery.REASONS[kind])
        self.assertEqual([r['what'] for r in rows if r['type'] == 'control'],
                         ['enroll', 'enroll', f'after {REPLAYED}', f'after {WRONG_KEY}'])
        replays = [r for r in rows if r['type'] == REPLAYED]
        self.assertEqual({r['variant'] for r in replays}, set(recovery.REPLAYS))
        self.assertEqual({r['variant'] for r in rows if r['type'] == WRONG_KEY}, set(recovery.SHAPES))
        # One attempt a target in each round.
        self.assertEqual(sorted(r['target'] for r in replays[:2]), [0, 1])
        self.assertEqual(recovery.verify(record, attacks.rows_bytes(rows), GENESIS, ENGINE), recovery.PASS)

    def test_a_node_that_admits_a_replay_fails(self):
        _, _, record = campaign(FakeNode(admit_replays=True))
        self.assertEqual(record['status'], recovery.FAIL)
        self.assertEqual(record['types'][REPLAYED]['counts']['admitted'], 4)
        self.assertEqual(record['successes'], 4)

    def test_a_node_that_counts_a_non_guardian_signer_fails(self):
        _, _, record = campaign(FakeNode(admit_outsiders=True))
        self.assertEqual(record['types'][WRONG_KEY]['status'], recovery.FAIL)
        self.assertEqual(record['types'][WRONG_KEY]['counts']['admitted'], 4)
        self.assertEqual(record['types'][REPLAYED]['status'], recovery.PASS)

    def test_a_refusal_for_another_reason_fails_the_type(self):
        _, _, record = campaign(FakeNode(log='Sponsor current authority mismatch or protection active'))
        self.assertEqual(record['status'], recovery.FAIL)
        self.assertEqual(record['types'][REPLAYED]['counts']['refused_other'], 4)

    def test_a_refusal_that_changed_the_account_is_a_success(self):
        _, _, record = campaign(FakeNode(touch=True))
        self.assertEqual(record['successes'], 8)
        self.assertEqual(record['status'], recovery.FAIL)

    def test_an_attempt_with_no_answer_is_made_again(self):
        _, rows, record = campaign(FakeNode(unavailable=2))
        self.assertEqual(record['status'], recovery.PASS, record['checks'])
        self.assertEqual(record['types'][REPLAYED]['counts']['transport'], 2)
        self.assertEqual(record['types'][REPLAYED]['counts']['refused'], 4)
        with self.assertRaisesRegex(recovery.Failed, 'could not be made 3 times'):
            campaign(FakeNode(unavailable=100), targets=1)

    def test_no_attempt_without_the_targets_enrolled(self):
        _, rows, record = campaign(FakeNode(fail_enroll=True))
        self.assertEqual([r['type'] for r in rows], ['control', 'control'])
        self.assertFalse(record['checks']['controls'])
        self.assertEqual(record['status'], recovery.FAIL)
        self.assertEqual({t['status'] for t in record['types'].values()}, {'not_run'})

    def test_the_replays_stop_before_their_operations_expire(self):
        with self.assertRaisesRegex(recovery.Failed, 'too soon for more replays'):
            campaign(FakeNode(lifetime=recovery.MARGIN + 3), attempts=10)

    def test_verify_recomputes_every_outcome_from_the_rows(self):
        _, rows, record = campaign()
        edited = dict(rows[2], outcome='refused_other')
        with self.assertRaisesRegex(recovery.Failed, 'not the ones the evidence names'):
            recovery.verify(record, attacks.rows_bytes(rows[:2] + [edited] + rows[3:]), GENESIS, ENGINE)
        body = {k: v for k, v in record.items() if k != 'sha256'}
        body['rows_sha256'] = attacks.digest(attacks.rows_bytes(rows[:2] + [edited] + rows[3:]))
        forged = dict(body, sha256=recovery.record_digest(body))
        with self.assertRaisesRegex(recovery.Failed, 'does not follow from its answer'):
            recovery.verify(forged, attacks.rows_bytes(rows[:2] + [edited] + rows[3:]), GENESIS, ENGINE)
        with self.assertRaisesRegex(recovery.Failed, 'another genesis'):
            recovery.verify(record, attacks.rows_bytes(rows), b'{}', ENGINE)

    def test_the_launch_page_counts_the_familys_fewest_type(self):
        _, rows, record = campaign(attempts=3)
        attempts, successes = results.attack_kpis('recovery', [record, record])
        self.assertEqual((attempts['id'], attempts['value'], attempts['status'], attempts['target']),
                         ('campaign.recovery.attempts', 6, 'in_progress', 3000))
        self.assertEqual((successes['value'], successes['status']), (0, 'in_progress'))
        families, sources, _ = results.attack_records([(json.dumps(record).encode(), attacks.rows_bytes(rows))],
                                                      GENESIS, ENGINE)
        self.assertEqual((list(families), sources[0]['tool'], sources[0]['status']),
                         (['recovery'], 'attacks.recovery', 'PASS'))


STANDIN_CLI = '#!' + sys.executable + '''
import json, sys
plan = json.load(open(%r))
print(plan.get('stdout', ''))
sys.stderr.write(plan.get('stderr', ''))
sys.exit(plan.get('code', 0))
'''


class Rpc:
    slots = threading.BoundedSemaphore(2)

    def __init__(self, answer=None):
        self.answer, self.asked = answer, []

    def one(self, label, method, params):
        self.asked.append((method, params))
        if isinstance(self.answer, Exception):
            raise self.answer
        return self.answer


class DeskChannel(unittest.TestCase):
    """The desk's reading of the CLI's reports, through a stand-in CLI."""

    def desk(self, stdout='', stderr='', code=0, rpc=None):
        work = Path(tempfile.mkdtemp())
        self.addCleanup(__import__('shutil').rmtree, work, True)
        plan = work / 'plan.json'
        plan.write_text(json.dumps({'stdout': stdout, 'stderr': stderr, 'code': code}))
        cli = work / 'dytallix'
        cli.write_text(STANDIN_CLI % str(plan))
        cli.chmod(cli.stat().st_mode | stat.S_IEXEC)
        return recovery.Desk(rpc or Rpc(), cli, {'PATH': '/usr/bin:/bin'}, work / 'pin.json', work, {}, {}), work

    def test_a_refused_submission_is_the_nodes_answer_and_no_report_is_transport(self):
        report = {'status': 'check_tx_rejected', 'committed': False,
                  'check_tx': {'code': 1, 'log': CONSUMED, 'codespace': '', 'transaction_id': 'aa',
                               'engine_hash': None, 'submitted': True}, 'output_version': 1}
        desk, work = self.desk(json.dumps(report), 'Error: CheckTx rejected the transaction', 1)
        self.assertEqual(desk.submit(work / 'tx.json'), {'code': 1, 'log': CONSUMED})
        report['check_tx'].update(code=0, log='', engine_hash='AB' * 32)
        desk, work = self.desk(json.dumps(report))
        self.assertEqual(desk.submit(work / 'tx.json'), {'code': 0, 'log': '', 'engine_hash': 'ab' * 32})
        desk, work = self.desk('', 'Error: the channel closed', 1)
        with self.assertRaisesRegex(recovery.Unavailable, 'channel closed'):
            desk.submit(work / 'tx.json')

    def test_committed_bytes_go_to_admission_directly_and_are_found_by_their_hash(self):
        rpc = Rpc({'code': 1, 'log': CONSUMED, 'codespace': ''})
        desk, work = self.desk(rpc=rpc)
        self.assertEqual(desk.check(b'{"kind":"recovery"}'), {'code': 1, 'log': CONSUMED})
        self.assertEqual(rpc.asked, [('check_tx', {'tx': base64.b64encode(b'{"kind":"recovery"}').decode()})])
        rpc.answer = {'log': ''}
        self.assertEqual(desk.check(b'x'), {'code': 0, 'log': ''})
        rpc.answer = recovery.load.Transport('the gateway: closed')
        with self.assertRaisesRegex(recovery.Unavailable, 'closed'):
            desk.check(b'x')
        tx = work / 'tx.json'
        tx.write_text(json.dumps({'envelope_base64': 'QUJD'}))
        sent = b'{"envelope_base64":"QUJD","kind":"recovery"}'
        self.assertEqual(recovery.wire_bytes(tx, hashlib.sha256(sent).hexdigest().upper()), sent)
        self.assertIsNone(recovery.wire_bytes(tx, 'aa' * 32))
        self.assertIsNone(recovery.wire_bytes(tx, None))

    def test_an_offline_command_that_fails_is_the_campaigns_fault(self):
        desk, work = self.desk('', 'Error: this key is not named for the operation role', 1)
        with self.assertRaisesRegex(recovery.Failed, 'not named'):
            desk.sign(work / 'op.json', 'operation', work / 'key.json', work, 'x')
        with self.assertRaisesRegex(recovery.Unavailable, 'not named'):
            desk.tool('query', node=True)

    def test_the_receipt_ids_and_the_fee_bounds(self):
        receipt = {'success': True, 'block_height': 7, 'retained_until': 17_287, 'gas_used': 180_000,
                   'settled_fee': 1_800_000, 'operation_id': list(range(32)),
                   'sponsor_authorization_id': [255] * 32}
        desk, work = self.desk(json.dumps({'status': 'committed_receipt_verified', 'height': 8, 'receipt': receipt}))
        found = desk.receipt(work / 'tx.json')
        self.assertEqual((found['operation_id'], found['authorization_id'], found['settled_fee']),
                         (bytes(range(32)).hex(), 'ff' * 32, '1800000'))
        fee = {'gas_price': '10', 'minimum_gas': '100000', 'max_transaction_gas': '2000000', 'max_fee_cap': '2000000'}
        self.assertEqual(recovery.fee_bounds(fee), (200_000, 2_000_000))
        with self.assertRaises(recovery.Failed):
            recovery.fee_bounds(dict(fee, max_fee_cap='900000'))
        tx = work / 'tx.json'
        tx.write_text(json.dumps({'envelope_base64': base64.b64encode(b'envelope').decode()}))
        self.assertEqual(recovery.envelope_digest(tx), hashlib.sha256(b'envelope').hexdigest())
        self.assertEqual(recovery.last_json('noise\n{"a": 1}\n'), {'a': 1})


if __name__ == '__main__':
    sys.exit(unittest.main())
