"""The attack campaign (staking family) without a chain: the outcome of each
answer and read, each attempt's fault as the desk reads it, and whole
campaigns against a stand-in node that admits and runs staking actions in
the node's order, including nodes that accept an attack, pay a repeated
claim, give another rule, change state on a failure or lose a transaction.
The staging network runs it for real in the Host install workflow."""
import base64
import copy
import json
import unittest

import attacks
import load
import results
import staking

GENESIS, ENGINE = b'{"native": 1}', b'{"engine": 1}'
MINIMUM = 100_000_000_000
VALIDATOR, CAMPAIGN = 'validator-1', 'campaign-validator'
REGISTERED_KEY = base64.b64encode(b'registered').decode()
FRESH_KEY = base64.b64encode(b'fresh').decode()
MATURITY = 242_640
CHARGE = 1_000
WALLETS = {'bond_to_unknown_validator': 'load-1', 'unbond_more_than_bonded': 'load-2',
           'early_withdrawal': 'load-3', 'repeated_reward_claim': 'load-4'}


def account(liquid_dgt=500, bonded=0, unbonds=()):
    bonds = [{'validator_id': VALIDATOR, 'amount_udgt': str(bonded)}] if bonded else []
    return {'context': {'height': 100}, 'liquid_udgt': str(liquid_dgt), 'liquid_udrt': '200000000', 'nonce': '1',
            'bonded_udgt': str(bonded), 'bonds': bonds, 'pending_bond_udgt': '0',
            'unbonding_udgt': str(sum(int(u['amount_udgt']) for u in unbonds)), 'unbonds': list(unbonds),
            'claimable_rewards_udrt': '0'}


class FakeDesk:
    """A stand-in node behind the desk's interface. CheckTx: the operator's
    approval, then the proof. Execution, in the node's order: registered,
    used key, zero, the minimum; a bond's target; an unbond's principal; a
    withdrawal's maturity. A failure charges the fee and spends the nonce."""

    def __init__(self, admits=(), pays_repeat=False, wrong_rule=None, leaks=None, lose=0, proof_late=False,
                 refuse_control=None, drop_at=None):
        self.admits, self.pays_repeat, self.wrong_rule, self.leaks = set(admits), pays_repeat, wrong_rule, leaks
        self.lose, self.proof_late, self.refuse_control = lose, proof_late, refuse_control
        # The send whose answers the channel loses, though the node took it.
        self.drop_at, self.sends = drop_at, 0
        self.height = 100
        self.minimum = MINIMUM
        self.set = [[VALIDATOR, REGISTERED_KEY]]
        unbond = {'unbond_id': 'u-1', 'validator_id': VALIDATOR, 'amount_udgt': '1000000', 'request_height': '90',
                  'effective_height': '92'}
        self.state = {'campaign-operator': account(liquid_dgt=MINIMUM),
                      'load-1': account(), 'load-2': account(bonded=100_000_000),
                      'load-3': account(bonded=99_000_000, unbonds=[unbond]), 'load-4': account()}
        self.pending, self.sent, self.rounds = {}, [], []
        self.first_claim = {'receipt': {'outcome': 'success', 'rule_code': None, 'charge': str(CHARGE),
                                        'nonce_before': 0, 'nonce_after': 1, 'block_height': 95},
                            'paid_udrt': '1234'}
        self.counter = 0

    # The desk's interface

    def wallet(self, kind):
        return 'campaign-operator' if kind in staking.REGISTRATIONS else WALLETS[kind]

    def accounts(self, labels):
        self.rounds.append(list(labels))
        return {label: dict(copy.deepcopy(self.state[label]), context={'height': self.height}) for label in labels}

    def validators(self):
        return copy.deepcopy(self.set)

    def forge(self, kind, k, before, validators):
        keys = {key for _, key in validators}
        if kind in staking.REGISTRATIONS:
            key = REGISTERED_KEY if kind == 'registration_reused_key' else FRESH_KEY
            amount = MINIMUM - 1 - k if kind == 'bond_below_minimum' else MINIMUM
            action = {'type': 'ValidatorRegister', 'validator_id': CAMPAIGN, 'key': key,
                      'proof_ok': kind != 'registration_bad_proof', 'amount_udgt': amount}
            fault = {'key': 'registered' if key in keys else 'fresh',
                     'proof': 'valid' if action['proof_ok'] else 'altered',
                     'amount_udgt': str(amount), 'minimum_udgt': str(MINIMUM)}
        elif kind == 'bond_to_unknown_validator':
            action = {'type': 'RewardBond', 'validator_id': f'unknown-validator-{k}', 'amount_udgt': 1 + k}
            fault = {'target_known': False, 'amount_udgt': str(1 + k)}
        elif kind == 'unbond_more_than_bonded':
            bonded = int(before['bonded_udgt'])
            action = {'type': 'RewardBeginUnbond', 'validator_id': VALIDATOR, 'amount_udgt': bonded + 1 + k}
            fault = {'target_known': True, 'bonded_udgt': str(bonded), 'amount_udgt': str(bonded + 1 + k)}
        elif kind == 'early_withdrawal':
            entry = before['unbonds'][0]
            action = {'type': 'ValidatorWithdraw', 'unbond_id': entry['unbond_id']}
            fault = {'owned': True, 'height': self.height, 'matures_at': int(entry['effective_height']) + MATURITY}
        else:
            action = {'type': 'RewardClaim'}
            fault = {'bonded_udgt': before['bonded_udgt'], 'claimed_before': True}
        self.counter += 1
        ident = f'tx-{self.counter}'
        return ident, json.dumps({'id': ident, 'kind': kind, 'wallet': self.wallet(kind), 'action': action}).encode(), \
            fault

    def admit(self, tx):
        action = tx['action']
        if action['type'] == 'ValidatorRegister' and not action['proof_ok'] and not self.proof_late:
            return 1, 'Ordinary transaction rejected: INVALID_VALIDATOR_PROOF'
        return 0, ''

    def send(self, txs):
        self.sends += 1
        if self.sends - 1 == self.drop_at:
            for raw in txs:
                tx = json.loads(raw)
                if self.admit(tx)[0] == 0:
                    self.execute(tx)
            raise staking.Unavailable('connection reset')
        answers = []
        for raw in txs:
            tx = json.loads(raw)
            self.sent.append(tx)
            code, log = self.admit(tx)
            if code == 0:
                self.pending[tx['id']] = tx
            answers.append({'code': code, 'log': log})
        return answers

    def rule(self, tx):
        """The node's rule for an action at execution, None if it applies."""
        if tx['kind'] in self.admits:
            return None
        action, state = tx['action'], self.state[tx['wallet']]
        if action['type'] == 'ValidatorRegister':
            if not action['proof_ok']:
                return 'INVALID_VALIDATOR_PROOF'
            if action['key'] in {key for _, key in self.set}:
                return 'CONSENSUS_KEY_ALREADY_USED'
            if action['amount_udgt'] < self.minimum:
                return 'VALIDATOR_SELF_BOND_MINIMUM'
            return None
        if action['type'] == 'RewardBond':
            return None if action['validator_id'] in {v for v, _ in self.set} else 'BOND_TARGET_UNAVAILABLE'
        if action['type'] == 'RewardBeginUnbond':
            return 'UNBOND_PRINCIPAL_UNAVAILABLE' if action['amount_udgt'] > int(state['bonded_udgt']) else None
        if action['type'] == 'ValidatorWithdraw':
            return 'UNBOND_NOT_MATURE'
        return None

    def execute(self, tx):
        state = self.state[tx['wallet']]
        before_nonce = int(state['nonce'])
        state['nonce'] = str(before_nonce + 1)
        state['liquid_udrt'] = str(int(state['liquid_udrt']) - CHARGE)
        code = self.rule(tx)
        if code and self.wrong_rule and tx['kind'] == self.wrong_rule:
            code = 'UNBOND_PENALTIES_PENDING'
        action = tx['action']
        if code is None:
            if action['type'] == 'ValidatorRegister':
                self.set.append([action['validator_id'], action['key']])
                state['liquid_udgt'] = str(int(state['liquid_udgt']) - action['amount_udgt'])
            elif action['type'] == 'RewardBond':
                state['liquid_udgt'] = str(int(state['liquid_udgt']) - action['amount_udgt'])
                state['pending_bond_udgt'] = str(int(state['pending_bond_udgt']) + action['amount_udgt'])
            elif action['type'] == 'RewardBeginUnbond':
                state['bonded_udgt'] = str(int(state['bonded_udgt']) - min(action['amount_udgt'],
                                                                           int(state['bonded_udgt'])))
            elif action['type'] == 'ValidatorWithdraw':
                state['unbonds'] = []
            elif action['type'] == 'RewardClaim' and self.pays_repeat:
                state['liquid_udrt'] = str(int(state['liquid_udrt']) + 50)
        elif self.leaks == tx['kind']:
            state['liquid_udgt'] = str(int(state['liquid_udgt']) - 1)
        return {'outcome': 'success' if code is None else 'application_failure', 'rule_code': code,
                'charge': str(CHARGE), 'nonce_before': before_nonce, 'nonce_after': before_nonce + 1,
                'block_height': self.height + 1}

    def wait(self, ids):
        self.height += 1
        found = {}
        for ident in ids:
            tx = self.pending.pop(ident)
            if self.lose:
                self.lose -= 1
                found[ident] = None
                continue
            found[ident] = self.execute(tx)
        return found

    def control(self, what):
        if what == self.refuse_control:
            return {'code': 1, 'log': 'refused', 'receipt': None}
        if what == 'registration':
            return {'code': 0, 'log': ''}
        self.height += 1
        return {'code': 0, 'log': '', 'receipt': {'outcome': 'success', 'rule_code': None, 'charge': str(CHARGE),
                                                  'nonce_before': 5, 'nonce_after': 6, 'block_height': self.height}}


def campaign(attempts=3, kinds=None, **knobs):
    desk = FakeDesk(**knobs)
    shown = []
    rows = staking.Campaign(desk, attempts, show=shown.append).run(kinds or list(staking.REASONS))
    record = staking.build_record('dytallix-staging-1', GENESIS, ENGINE, rows, attempts, {'minimum_udgt': str(MINIMUM)},
                                  '2026-11-05T00:00:00Z', '2026-11-05T00:10:00Z')
    return desk, rows, record, shown


def executed(kind, outcome, rule, fault, *, changed=(), nonce_step=1, drt_change=-CHARGE, claimable=('0', '0'),
             validators_changed=None):
    row = {'type': kind, 'k': 0, 'wallet': 'w', 'fault': fault, 'code': 0, 'log': '',
           'receipt': {'outcome': outcome, 'rule_code': rule, 'charge': str(CHARGE), 'nonce_before': 4,
                       'nonce_after': 5, 'block_height': 9},
           'state': {'changed': list(changed), 'nonce_step': nonce_step, 'drt_change': str(drt_change),
                     'claimable': list(claimable)}}
    if validators_changed is not None:
        row['state']['validators_changed'] = validators_changed
    return row


UNKNOWN = {'target_known': False, 'amount_udgt': '1'}
CLAIM = {'bonded_udgt': '0', 'claimed_before': True}
BELOW = {'key': 'fresh', 'proof': 'valid', 'amount_udgt': str(MINIMUM - 1), 'minimum_udgt': str(MINIMUM)}


class Outcomes(unittest.TestCase):
    def test_a_failure_counts_only_for_its_rule_with_nothing_changed_but_the_fee(self):
        ok = executed('bond_to_unknown_validator', 'application_failure', 'BOND_TARGET_UNAVAILABLE', UNKNOWN,
                      validators_changed=False)
        self.assertEqual(staking.classify(ok), 'refused')
        for wrong, outcome in (
                (dict(ok, receipt=dict(ok['receipt'], rule_code='UNBOND_PENALTIES_PENDING')), 'refused_other'),
                (dict(ok, receipt=dict(ok['receipt'], outcome='out_of_gas')), 'refused_other'),
                (dict(ok, receipt=dict(ok['receipt'], outcome='success')), 'admitted'),
                (dict(ok, state=dict(ok['state'], changed=['liquid_udgt'])), 'admitted'),
                (dict(ok, state=dict(ok['state'], drt_change=str(-CHARGE + 1))), 'admitted'),
                (dict(ok, state=dict(ok['state'], nonce_step=2)), 'admitted'),
                (dict(ok, state=dict(ok['state'], validators_changed=True)), 'admitted'),
                (dict(ok, fault=dict(UNKNOWN, target_known=True)), 'refused_other'),
                (dict(ok, receipt=None, state=None), 'transport'),
                ({k: v for k, v in ok.items() if k != 'code'}, 'transport')):
            self.assertEqual(staking.classify(wrong), outcome)

    def test_a_refusal_at_admission_counts_for_the_types_rule(self):
        bad = {'key': 'fresh', 'proof': 'altered', 'amount_udgt': str(MINIMUM), 'minimum_udgt': str(MINIMUM)}
        row = {'type': 'registration_bad_proof', 'k': 0, 'wallet': 'w', 'fault': bad, 'code': 1,
               'log': 'Ordinary transaction rejected: INVALID_VALIDATOR_PROOF'}
        self.assertEqual(staking.classify(row), 'refused')
        self.assertEqual(staking.classify(dict(row, log='VALIDATOR_OPERATOR_REQUIRED')), 'refused_other')
        self.assertEqual(staking.classify(dict(row, fault=dict(bad, proof='valid'))), 'refused_other')
        self.assertEqual(staking.classify(dict(row, fault=dict(bad, amount_udgt=str(MINIMUM - 1)))),
                         'refused_other')

    def test_each_attempt_counts_only_when_it_carries_its_types_fault(self):
        rule = staking.REASONS
        for kind, fault, wrong in (
                ('bond_below_minimum', BELOW, dict(BELOW, amount_udgt=str(MINIMUM))),
                ('bond_below_minimum', BELOW, dict(BELOW, key='registered')),
                ('registration_reused_key', dict(BELOW, key='registered', amount_udgt=str(MINIMUM)),
                 dict(BELOW, key='registered')),
                ('unbond_more_than_bonded', {'target_known': True, 'bonded_udgt': '5', 'amount_udgt': '6'},
                 {'target_known': True, 'bonded_udgt': '5', 'amount_udgt': '5'}),
                ('unbond_more_than_bonded', {'target_known': True, 'bonded_udgt': '5', 'amount_udgt': '6'},
                 {'target_known': True, 'bonded_udgt': '0', 'amount_udgt': '6'}),
                ('early_withdrawal', {'owned': True, 'height': 10, 'matures_at': 11},
                 {'owned': True, 'height': 11, 'matures_at': 11}),
                ('early_withdrawal', {'owned': True, 'height': 10, 'matures_at': 11},
                 {'owned': False, 'height': 10, 'matures_at': 0})):
            self.assertEqual(staking.classify(executed(kind, 'application_failure', rule[kind], fault)), 'refused',
                             kind)
            self.assertEqual(staking.classify(executed(kind, 'application_failure', rule[kind], wrong)),
                             'refused_other', (kind, wrong))

    def test_a_repeated_claim_counts_when_it_pays_nothing(self):
        row = executed('repeated_reward_claim', 'success', None, CLAIM)
        self.assertEqual(staking.classify(row), 'refused')
        self.assertEqual(staking.classify(executed('repeated_reward_claim', 'success', None, CLAIM,
                                                   drt_change=-CHARGE + 7)), 'admitted')
        self.assertEqual(staking.classify(executed('repeated_reward_claim', 'success', None, CLAIM,
                                                   claimable=('0', '3'))), 'admitted')
        self.assertEqual(staking.classify(executed('repeated_reward_claim', 'application_failure', 'X', CLAIM)),
                         'refused_other')
        self.assertEqual(staking.classify(executed('repeated_reward_claim', 'success', None,
                                                   dict(CLAIM, bonded_udgt='1'))), 'refused_other')
        self.assertEqual(staking.classify(executed('repeated_reward_claim', 'success', None,
                                                   dict(CLAIM, claimed_before=False))), 'refused_other')

    def test_the_reads_leave_out_claimable_rewards_but_keep_them(self):
        before = account(bonded=5)
        after = dict(before, nonce='2', liquid_udrt=str(200000000 - CHARGE), claimable_rewards_udrt='9')
        facts = staking.state_facts(before, after, [['a', 'k']], [['a', 'k']])
        self.assertEqual(facts, {'changed': [], 'nonce_step': 1, 'drt_change': str(-CHARGE),
                                 'claimable': ['0', '9'], 'validators_changed': False})
        moved = staking.state_facts(before, dict(after, bonds=[], unbonding_udgt='5'))
        self.assertEqual(moved['changed'], ['bonds', 'unbonding_udgt'])
        self.assertNotIn('validators_changed', moved)


class Campaigns(unittest.TestCase):
    def test_a_node_refuses_every_type_for_its_reason(self):
        desk, rows, record, shown = campaign()
        self.assertEqual(record['status'], 'PASS', record['types'])
        self.assertEqual({k: t['counts']['refused'] for k, t in record['types'].items()},
                         {k: 3 for k in staking.REASONS})
        self.assertEqual(record['successes'], 0)
        self.assertEqual(record['controls'], {'made': 5, 'passed': 5})
        self.assertEqual(record['types']['bond_below_minimum']['first_log'], 'VALIDATOR_SELF_BOND_MINIMUM')
        self.assertIn('INVALID_VALIDATOR_PROOF', record['types']['registration_bad_proof']['first_log'])
        self.assertEqual(staking.verify(record, attacks.rows_bytes(rows), GENESIS, ENGINE), 'PASS')
        self.assertTrue(any('the node said' in line for line in shown))

    def test_each_round_has_one_attempt_a_wallet_and_the_registrations_take_turns(self):
        desk, rows, _, _ = campaign(attempts=4)
        self.assertTrue(all(len(r) == len(set(r)) for r in desk.rounds))
        bad = [r for r in rows if r['type'] == 'registration_bad_proof']
        operator = [r['type'] for r in rows if r.get('wallet') == 'campaign-operator' and r['type'] != 'control']
        # The bad proofs first, one at a time; then the other two alternate.
        self.assertEqual(operator[:4], ['registration_bad_proof'] * 4)
        self.assertEqual(operator[4:], ['bond_below_minimum', 'registration_reused_key'] * 4)
        self.assertEqual([r['k'] for r in bad], [0, 1, 2, 3])

    def test_a_node_that_accepts_an_attack_fails_and_the_type_stops(self):
        desk, rows, record, _ = campaign(admits={'bond_to_unknown_validator'})
        t = record['types']['bond_to_unknown_validator']
        self.assertEqual((t['status'], t['counts']['admitted'], t['counts']['refused']), ('FAIL', 1, 0))
        self.assertEqual((record['status'], record['successes']), ('FAIL', 1))
        self.assertEqual(record['types']['early_withdrawal']['status'], 'PASS')

    def test_an_accepted_registration_stops_all_three(self):
        desk, rows, record, _ = campaign(admits={'bond_below_minimum'})
        self.assertEqual(record['types']['bond_below_minimum']['counts']['admitted'], 1)
        made = [r['type'] for r in rows if r['type'] in staking.REGISTRATIONS]
        self.assertEqual(made.count('registration_reused_key'), 0)
        self.assertEqual(made.count('registration_bad_proof'), 3)
        self.assertIn([CAMPAIGN, FRESH_KEY], desk.set)
        self.assertEqual(record['status'], 'FAIL')

    def test_a_repeated_claim_that_pays_is_a_success(self):
        _, _, record, _ = campaign(pays_repeat=True)
        t = record['types']['repeated_reward_claim']
        self.assertEqual((t['status'], t['counts']['admitted']), ('FAIL', 1))

    def test_another_rule_or_a_failure_that_moves_state_fails_the_type(self):
        _, _, record, _ = campaign(wrong_rule='early_withdrawal')
        t = record['types']['early_withdrawal']
        self.assertEqual((t['status'], t['counts']['refused_other']), ('FAIL', 3))
        self.assertEqual(t['first_log'], 'UNBOND_PENALTIES_PENDING')
        _, _, record, _ = campaign(leaks='unbond_more_than_bonded')
        t = record['types']['unbond_more_than_bonded']
        self.assertEqual((t['status'], t['counts']['admitted']), ('FAIL', 1))

    def test_a_bad_proof_refused_when_the_block_runs_still_counts(self):
        _, _, record, _ = campaign(proof_late=True)
        t = record['types']['registration_bad_proof']
        self.assertEqual((t['status'], t['counts']['refused'], t['first_log']), ('PASS', 3, 'INVALID_VALIDATOR_PROOF'))

    def test_an_attempt_that_never_committed_is_made_again(self):
        _, rows, record, _ = campaign(lose=2)
        self.assertEqual(sum(1 for r in rows if r.get('outcome') == 'transport'), 2)
        self.assertEqual(record['status'], 'PASS')
        self.assertEqual(staking.verify(record, attacks.rows_bytes(rows), GENESIS, ENGINE), 'PASS')
        with self.assertRaisesRegex(staking.Failed, 'could not be made 3 times running'):
            campaign(lose=100)

    def test_a_lost_answer_whose_attempt_committed_counts_as_admitted(self):
        _, rows, record, _ = campaign(drop_at=3)
        moved = [r for r in rows if r.get('moved_nonce')]
        self.assertEqual(sorted(r['wallet'] for r in moved), ['campaign-operator'] + sorted(WALLETS.values()))
        self.assertTrue(all(r['outcome'] == 'admitted' and 'code' not in r for r in moved))
        self.assertEqual((record['successes'], record['status']), (5, 'FAIL'))
        self.assertEqual(staking.verify(record, attacks.rows_bytes(rows), GENESIS, ENGINE), 'FAIL')
        n = rows.index(moved[0])
        unmoved = rows[:n] + [{k: v for k, v in rows[n].items() if k != 'moved_nonce'}] + rows[n + 1:]
        with self.assertRaisesRegex(staking.Failed, 'does not follow'):
            staking.build_record('c', GENESIS, ENGINE, unmoved, 3, {}, 's', 'f')
        # Only an attempt that was lost can be counted so.
        r = next(i for i, row in enumerate(rows) if row.get('outcome') == 'refused')
        forged = rows[:r] + [dict(rows[r], outcome='admitted', moved_nonce=True)] + rows[r + 1:]
        with self.assertRaisesRegex(staking.Failed, 'does not follow'):
            staking.build_record('c', GENESIS, ENGINE, forged, 3, {}, 's', 'f')

    def test_a_nonce_moved_with_nothing_lost_stops_the_campaign(self):
        desk = FakeDesk()
        campaign_ = staking.Campaign(desk, 3, show=lambda _: None)
        campaign_.last = {'load-1': 0}
        with self.assertRaisesRegex(staking.Failed, 'load-1\'s nonce moved from 0 to 1 with no attempt'):
            campaign_.guard({'load-1': account()})

    def test_a_control_the_node_refuses_fails_the_record(self):
        _, _, record, shown = campaign(refuse_control='bond')
        self.assertEqual((record['checks']['controls'], record['status']), (False, 'FAIL'))
        self.assertTrue(any('positive control bond' in line for line in shown))

    def test_verify_recomputes_every_outcome_from_the_rows(self):
        _, rows, record = campaign(attempts=2)[:3]
        raw = attacks.rows_bytes(rows)
        n = next(i for i, r in enumerate(rows) if r['type'] == 'early_withdrawal')
        edited = rows[:n] + [dict(rows[n], outcome='admitted')] + rows[n + 1:]
        with self.assertRaisesRegex(staking.Failed, 'rows are not the ones'):
            staking.verify(record, attacks.rows_bytes(edited), GENESIS, ENGINE)
        with self.assertRaisesRegex(staking.Failed, 'another genesis'):
            staking.verify(record, raw, b'{}', ENGINE)
        with self.assertRaisesRegex(staking.Failed, 'digest does not match'):
            staking.verify(dict(record, attempts_wanted=1), raw, GENESIS, ENGINE)
        body = {k: v for k, v in record.items() if k != 'sha256'}
        body['rows_sha256'] = attacks.digest(attacks.rows_bytes(edited))
        forged = dict(body, sha256=staking.record_digest(body))
        with self.assertRaisesRegex(staking.Failed, 'does not follow from its answers'):
            staking.verify(forged, attacks.rows_bytes(edited), GENESIS, ENGINE)

    def test_the_launch_page_counts_the_familys_fewest_type(self):
        _, rows, record, _ = campaign(attempts=3)
        attempts, successes = results.attack_kpis('staking', [record, record])
        self.assertEqual((attempts['id'], attempts['value'], attempts['status'], attempts['target']),
                         ('campaign.staking.attempts', 6, 'in_progress', 3000))
        self.assertEqual((successes['value'], successes['status']), (0, 'in_progress'))
        families, sources, _ = results.attack_records([(json.dumps(record).encode(), attacks.rows_bytes(rows))],
                                                      GENESIS, ENGINE)
        self.assertEqual((list(families), sources[0]['status']), (['staking'], 'PASS'))


class StubRun:
    """The parts of load.Load the desk signs through."""

    def __init__(self):
        self.s = load.Settings()
        self.profile = {'context': {'height': 50}}
        self.signed, self.synced = [], []

    def refresh_profile(self):
        pass

    def sync(self, wallets):
        self.synced.append([w.label for w in wallets])

    def sign(self, row, wallet, actions, gas_limit=load.GAS_LIMIT):
        self.signed.append((wallet.label, actions, gas_limit))
        row.update(id=f'id-{len(self.signed)}', expiry_height=71)
        return row, b'tx'


def query_result(value):
    return {'response': {'code': 0, 'value': base64.b64encode(json.dumps(value).encode()).decode()}}


class StubRpc:
    """The client channel: each method answered by a function of its params."""

    def __init__(self, **answers):
        self.answers, self.calls = answers, []

    def call(self, label, requests):
        if not 0 < len(requests) <= load.MAX_BATCH:
            raise load.Failed('batch size')
        self.calls.append([m for m, _ in requests])
        return [self.answers[m](p) for m, p in requests]

    def one(self, label, method, params):
        result = self.call(label, [(method, params)])[0]
        if isinstance(result, load.RpcError):
            raise result
        return result


class DeskCase:
    """A real desk over a stub run (and channel)."""

    def desk(self, rpc=None):
        operator = load.Wallet('campaign-operator', 'operator', 'dytallix1op', 'aa' * 32, keystore_name='op')
        operator.nonce = 7
        wallets = {kind: load.Wallet(label, 'load', f'dytallix1{label}', 'bb' * 32)
                   for kind, label in WALLETS.items()}
        desk = staking.Desk(StubRun(), rpc, 'keytool', 'genesis.json', operator, CAMPAIGN, VALIDATOR,
                            '/var/lib/key.json', '/out/fresh.json', MATURITY, wallets, sleep=lambda _: None)
        desk.minimum = MINIMUM
        public = {'registered': b'registered', 'fresh': b'fresh'}
        desk.proofs = []

        def proof(key, nonce, expiry, amount):
            desk.proofs.append((key, nonce, expiry, amount))
            return {'type': 'ValidatorRegister', 'validator_id': CAMPAIGN, 'consensus_key': list(public[key]),
                    'proof': [0] * staking.PROOF_BYTES, 'proof_expiry_height': str(expiry), 'amount_udgt': str(amount)}
        desk.proof = proof
        return desk


class DeskForging(DeskCase, unittest.TestCase):
    def test_each_registration_carries_its_fault_as_the_chain_shows_it(self):
        desk = self.desk()
        before = account(liquid_dgt=MINIMUM)
        validators = [[VALIDATOR, REGISTERED_KEY]]
        _, _, fault = desk.forge('bond_below_minimum', 2, before, validators)
        self.assertEqual(fault, {'key': 'fresh', 'proof': 'valid', 'amount_udgt': str(MINIMUM - 3),
                                 'minimum_udgt': str(MINIMUM)})
        self.assertEqual(desk.proofs[-1], ('fresh', 7, 71, MINIMUM - 3))
        self.assertEqual(desk.run.signed[-1][2], load.FUNDING_GAS_LIMIT)
        _, _, fault = desk.forge('registration_reused_key', 0, before, validators)
        self.assertEqual((fault['key'], fault['amount_udgt']), ('registered', str(MINIMUM)))
        _, _, fault = desk.forge('registration_bad_proof', staking.PROOF_BYTES + 5, before, validators)
        action = desk.run.signed[-1][1][0]
        self.assertEqual((fault['key'], fault['proof']), ('fresh', 'altered'))
        self.assertEqual([(n, b) for n, b in enumerate(action['proof']) if b], [(5, 2)])
        with self.assertRaisesRegex(staking.Failed, 'the registered key is fresh on the chain'):
            desk.forge('registration_reused_key', 0, before, [])

    def test_each_delegation_attempt_reads_its_fault_from_the_wallet(self):
        desk = self.desk()
        desk.unbond_id, desk.first_claim = 'u-1', {}
        validators = [[VALIDATOR, REGISTERED_KEY]]
        _, _, fault = desk.forge('bond_to_unknown_validator', 4, account(), validators)
        self.assertEqual(fault, {'target_known': False, 'amount_udgt': '5'})
        self.assertEqual(desk.run.signed[-1][1], [{'type': 'RewardBond', 'validator_id': 'unknown-validator-4',
                                                   'amount_udgt': '5'}])
        _, _, fault = desk.forge('unbond_more_than_bonded', 1, account(bonded=30), validators)
        self.assertEqual(fault, {'target_known': True, 'bonded_udgt': '30', 'amount_udgt': '32'})
        entry = {'unbond_id': 'u-1', 'validator_id': VALIDATOR, 'amount_udgt': '1', 'request_height': '90',
                 'effective_height': '92'}
        _, _, fault = desk.forge('early_withdrawal', 0, account(unbonds=[entry]), validators)
        self.assertEqual(fault, {'owned': True, 'height': 100, 'matures_at': 92 + MATURITY})
        _, _, fault = desk.forge('early_withdrawal', 0, account(), validators)
        self.assertEqual(fault['owned'], False)
        _, _, fault = desk.forge('repeated_reward_claim', 0, account(), validators)
        self.assertEqual(fault, {'bonded_udgt': '0', 'claimed_before': True})


class DeskChannel(DeskCase, unittest.TestCase):
    """The desk's requests through a stand-in channel."""

    def receipt(self, ident, outcome='application_failure', rule='UNBOND_NOT_MATURE'):
        return {'transaction_id': ident, 'outcome': outcome, 'rule_code': rule, 'charge': '1350000',
                'nonce_before': '4', 'nonce_after': '5', 'block_height': '120', 'actor': 'ab' * 32}

    def test_answers_and_refusals_before_admission(self):
        def broadcast(params):
            return load.RpcError('tx too large') if len(base64.b64decode(params['tx'])) > 3 else {'code': 3, 'log': 'x'}
        desk = self.desk(StubRpc(broadcast_tx_sync=broadcast))
        self.assertEqual(desk.send([b'big tx', b'ok']), [{'code': -1, 'log': 'tx too large'}, {'code': 3, 'log': 'x'}])

        def lost(params):
            raise load.Transport('reset')
        with self.assertRaises(staking.Unavailable):
            self.desk(StubRpc(broadcast_tx_sync=lost)).send([b'tx'])

    def test_a_receipt_comes_or_its_expiry_passes(self):
        heads = iter([70, 71, 72])
        receipts = {'/ordinary/receipt/a': self.receipt('a'), '/ordinary/receipt/b': None}
        desk = self.desk(StubRpc(abci_query=lambda p: query_result(receipts[p['path']]),
                                 abci_info=lambda p: {'response': {'last_block_height': str(next(heads))}}))
        desk.expiries = {'a': 71, 'b': 71}
        found = desk.wait(['a', 'b'])
        self.assertEqual(found, {'a': {'outcome': 'application_failure', 'rule_code': 'UNBOND_NOT_MATURE',
                                       'charge': '1350000', 'nonce_before': 4, 'nonce_after': 5,
                                       'block_height': 120}, 'b': None})
        self.assertNotIn('actor', json.dumps(found))
        receipts['/ordinary/receipt/a'] = self.receipt('other')
        with self.assertRaisesRegex(staking.Failed, 'for another transaction'):
            desk.wait(['a'])

    def test_accounts_and_the_registration_control(self):
        summaries = {'/account/dytallix1op': account(liquid_dgt=MINIMUM), '/account/dytallix1load-4': account()}
        view = {'validators': [{'validator_id': VALIDATOR, 'owner': 'o', 'consensus_key_base64': REGISTERED_KEY,
                                'power_udgt': '9'}], 'min_self_bond_udgt': str(MINIMUM)}
        checked = []

        def query(params):
            return query_result(view if params['path'] == '/staking/validators' else summaries[params['path']])

        def check(params):
            checked.append(params['tx'])
            return {'log': ''}  # a zero code may be left out
        desk = self.desk(StubRpc(abci_query=query, check_tx=check))
        found = desk.accounts(['campaign-operator', 'load-4'])
        self.assertEqual(found, {'campaign-operator': summaries['/account/dytallix1op'],
                                 'load-4': summaries['/account/dytallix1load-4']})
        self.assertEqual(desk.run.synced, [['campaign-operator', 'load-4']])
        self.assertEqual(desk.validators(), [[VALIDATOR, REGISTERED_KEY]])
        self.assertEqual(desk.control('registration'), {'code': 0, 'log': ''})
        self.assertEqual(desk.proofs[-1], ('fresh', 7, 71, MINIMUM))
        self.assertEqual(len(checked), 1)
        self.assertNotIn(['broadcast_tx_sync'], desk.rpc.calls)



class SlowChain:
    """A stand-in chain for the setup: a bond or unbond takes effect two
    blocks after it commits, as the node's do, and each block pays every
    bonded wallet. `leak` keeps paying the claimer after it unbonds."""

    def __init__(self, desk, leak=False):
        self.desk, self.leak, self.height, self.pending, self.receipts, self.sent = desk, leak, 100, [], {}, []
        self.state = {w.address: {'liquid_udrt': 0 if w is desk.operator else 10 ** 12, 'bonded': 0,
                                  'unbonds': [], 'claimable': 0} for w in desk.wallets.values()}
        desk.summary, desk.head, desk.online, desk.sleep = self.summary, lambda: self.height, self.online, self.tick
        desk.run.prime, desk.run.gas_price = (lambda pairs: None), 10
        desk.run.funder = load.Wallet('ecosystem-growth', 'funder', 'dytallix1funder', 'cc' * 32)
        self.state['dytallix1funder'] = {'liquid_udrt': 10 ** 15, 'bonded': 0, 'unbonds': [], 'claimable': 0}
        desk.rpc = StubRpc(abci_query=lambda p: query_result(self.receipts[p['path']]))

    def tick(self, _=None):
        self.height += 1
        for when, effect in [p for p in self.pending if p[0] == self.height]:
            effect()
        for address, s in self.state.items():
            if s['bonded'] or (self.leak and address == 'dytallix1load-4'):
                s['claimable'] += 5

    def summary(self, wallet):
        s = self.state[wallet.address]
        return {'liquid_udrt': str(s['liquid_udrt']), 'bonded_udgt': str(s['bonded']), 'unbonds': list(s['unbonds']),
                'claimable_rewards_udrt': str(s['claimable'])}

    def online(self, wallet, *args):
        self.tick()
        s, ident = self.state[wallet.address], f'tx-{len(self.sent)}'
        self.sent.append((wallet.label, args))
        amount = int(args[-1]) * load.UNIT if args[0] == 'stake' and args[1] != 'claim' else 0
        if args[0] == 'send':
            self.state[args[2]]['liquid_udrt'] += int(args[4]) * load.UNIT
        elif args[1] == 'bond':
            self.pending.append((self.height + 2, lambda: s.update(bonded=s['bonded'] + amount)))
        elif args[1] == 'unbond':
            def effect():
                s.update(bonded=s['bonded'] - amount)
                s['unbonds'].append({'unbond_id': f'u-{ident}', 'validator_id': VALIDATOR, 'amount_udgt': str(amount)})
            self.pending.append((self.height + 2, effect))
        else:
            s.update(liquid_udrt=s['liquid_udrt'] + s['claimable'], claimable=0)
        s['liquid_udrt'] -= CHARGE
        self.receipts[f'/ordinary/receipt/{ident}'] = {
            'transaction_id': ident, 'outcome': 'success', 'rule_code': None, 'charge': str(CHARGE),
            'nonce_before': '1', 'nonce_after': '2', 'block_height': str(self.height)}
        return {'id': ident, 'charge_udrt': str(CHARGE)}


class DeskSetup(DeskCase, unittest.TestCase):
    def test_the_setup_waits_for_each_bond_and_unbond_to_take_effect(self):
        desk = self.desk()
        chain = SlowChain(desk)
        desk.prepare(2)
        need = (2 * 2 + 10) * load.FUNDING_GAS_LIMIT * 10 // load.UNIT
        self.assertEqual(chain.sent[0], ('ecosystem-growth', ('send', '--to', 'dytallix1op', '--amount', str(need),
                                                              '--token', 'drt')))
        early = chain.state['dytallix1load-3']
        self.assertEqual(desk.unbond_id, early['unbonds'][0]['unbond_id'])
        self.assertEqual(early['bonded'], (staking.BOND_DGT - staking.EARLY_UNBOND_DGT) * load.UNIT)
        self.assertGreater(int(desk.first_claim['paid_udrt']), 0)
        self.assertEqual(desk.first_claim['receipt']['outcome'], 'success')
        self.assertEqual(chain.state['dytallix1load-4']['bonded'], 0)

    def test_a_claimer_still_paid_after_unbonding_fails_the_setup(self):
        desk = self.desk()
        SlowChain(desk, leak=True)
        with self.assertRaisesRegex(staking.Failed, 'still accrues rewards'):
            desk.prepare(2)


if __name__ == '__main__':
    unittest.main()
