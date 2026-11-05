"""The attack campaign (governance family) without a chain: the outcome of
each answer and read, and whole campaigns through the real desk against a
stand-in node that keeps governance as the node's store does (the next
proposal ID, the deposit close, the vote's snapshot, a finished proposal
kept one block, then removed; the rules in the node's order), including
nodes that accept an out-of-bounds change, open a vote below the minimum,
keep a deposit at its close, count a second vote or lose a transaction.
The staging network runs it for real in the Host install workflow."""
import base64
import copy
import hashlib
import json
import threading
import unittest

import governance
import load
import results

GENESIS, ENGINE = b'{"native": 1}', b'{"engine": 1}'
MINIMUM = 10_000_000_000
BOUNDS = {'max_active': {'min': 4, 'max': 32}, 'min_self_bond': {'min': 10_000_000_000, 'max': 1_000_000_000_000},
          'gas_price': {'min': 1, 'max': 100}}
GAS_PRICE, MINIMUM_GAS, GAS_USED = 10, 100_000, 47_000
CHARGE = MINIMUM_GAS * GAS_PRICE
BOND = 100_000_000
PROPOSER, DEPOSITOR = 'gov-proposer', 'gov-depositor'
PARAMETER, REPEATERS, LOADS = ['gov-voter-1', 'gov-voter-2'], ['gov-voter-3', 'gov-voter-4'], ['load-1', 'load-2']
ROLES = {'proposer': PROPOSER, 'depositor': DEPOSITOR, governance.BELOW: [PROPOSER],
         governance.OUT_OF_BOUNDS: PARAMETER, governance.LATE: PARAMETER, governance.REPEATED: REPEATERS,
         governance.UNBONDED: LOADS}


def config(deposit_period=120_960, voting_period=120_960):
    return {'governance': {'deposit': {'deposit_period_blocks': deposit_period, 'minimum_deposit_udgt': MINIMUM},
                           'ballot': {'voting_period_blocks': voting_period}, 'parameter_bounds': BOUNDS},
            'lifecycle': {'max_active': 16}, 'ordinary': {'fee_profile': {'gas_price': GAS_PRICE}}}


# Outcomes

def state(nonce_step=1, drt_change=-CHARGE, udgt_change=0, changed=()):
    return {'changed': list(changed), 'nonce_step': nonce_step, 'drt_change': str(drt_change),
            'udgt_change': str(udgt_change), 'heights': [10, 12]}


def result(code=3, log='GOVERNANCE_PARAMETER_OUT_OF_BOUNDS', height=12):
    return {'code': code, 'log': log, 'gas_used': GAS_USED, 'height': height, 'charge': str(CHARGE)}


def out_of_bounds(value='33', bonded=True, proposal_id='5', **row):
    fault = {'parameter': 'max_active', 'value': value, 'bounds': [4, 32], 'bonded': bonded,
             'next_proposal_id': '5', 'proposal_id': proposal_id}
    base = {'type': governance.OUT_OF_BOUNDS, 'k': 0, 'wallet': PARAMETER[0], 'fault': fault, 'code': 0, 'log': '',
            'result': result(), 'state': state(), 'effect': {'what': 'proposal', 'before': False, 'after': False}}
    return {**base, **row}


def vote(kind, log, *, weight='100', voted=None, height=50, end=80, landed=60, **row):
    fault = {'proposal_id': '9', 'phase': 'voting' if kind != governance.LATE else None, 'snapshot_height': 40,
             'end_height': end, 'height': height, 'weight': weight, 'voted': voted}
    base = {'type': kind, 'k': 0, 'wallet': 'w', 'fault': fault, 'code': 0, 'log': '',
            'result': result(log=log, height=landed), 'state': state(),
            'effect': {'what': 'vote', 'before': voted, 'after': voted}}
    return {**base, **row}


def below(amount=2, phase='collecting', deposited=None, refunds=(), udgt_change=None, **row):
    deposit = amount
    charges = 2 if deposit else 1
    fault = {'deposit_udgt': str(deposit), 'minimum_udgt': str(MINIMUM), 'bonded': True, 'next_proposal_id': '5',
             'proposal_id': '5'}
    base = {'type': governance.BELOW, 'k': 1, 'wallet': PROPOSER, 'fault': fault, 'code': 0, 'log': '',
            'result': result(code=0, log=''),
            'proposal_view': {'phase': phase, 'deposited_udgt': str(deposit if deposited is None else deposited),
                              'close_height': 200, 'admitted_height': 11},
            'state': state(nonce_step=charges, drt_change=-charges * CHARGE,
                           udgt_change=(sum(int(r['udgt']) for r in refunds) - deposit
                                        if udgt_change is None else udgt_change)),
            'refunds': list(refunds)}
    if deposit:
        base['deposit'] = {'code': 0, 'log': '', 'result': result(code=0, log='')}
    return {**base, **row}


class Outcomes(unittest.TestCase):
    def test_an_out_of_bounds_change_counts_only_for_its_rule_with_nothing_changed_but_the_fee(self):
        self.assertEqual(governance.classify(out_of_bounds()), 'refused')
        self.assertEqual(governance.classify(out_of_bounds(result=result(code=0, log=''))), 'admitted')
        self.assertEqual(governance.classify(out_of_bounds(result=result(log='GOVERNANCE_PROPOSAL_ID'))),
                         'refused_other')
        self.assertEqual(governance.classify(out_of_bounds(state=state(drt_change=-2 * CHARGE))), 'admitted')
        self.assertEqual(governance.classify(out_of_bounds(state=state(changed=['bonded_udgt']))), 'admitted')
        stored = {'what': 'proposal', 'before': False, 'after': True}
        self.assertEqual(governance.classify(out_of_bounds(effect=stored)), 'admitted')
        # Refused at admission is not the type's refusal; no answer is transport.
        self.assertEqual(governance.classify(out_of_bounds(code=1, log='Governance spending nonce is stale')),
                         'refused_other')
        transport = out_of_bounds(error='the gateway: timed out')
        del transport['code']
        self.assertEqual(governance.classify(transport), 'transport')
        self.assertEqual(governance.classify(out_of_bounds(result=None)), 'transport')

    def test_each_attempt_counts_only_when_it_carries_its_types_fault(self):
        self.assertEqual(governance.classify(out_of_bounds(value='32')), 'refused_other')
        self.assertEqual(governance.classify(out_of_bounds(value='3')), 'refused')
        self.assertEqual(governance.classify(out_of_bounds(bonded=False)), 'refused_other')
        self.assertEqual(governance.classify(out_of_bounds(proposal_id='4')), 'refused_other')
        self.assertEqual(governance.classify(vote(governance.UNBONDED, 'GOVERNANCE_VOTER_NOT_ELIGIBLE')),
                         'refused_other')  # it has a weight: not a vote without a bond
        self.assertEqual(governance.classify(vote(governance.REPEATED, 'GOVERNANCE_ALREADY_VOTED')),
                         'refused_other')  # it never voted
        late = vote(governance.LATE, 'GOVERNANCE_PROPOSAL_UNKNOWN', height=80, landed=81)
        self.assertEqual(governance.classify(late), 'refused_other')  # signed at the end height, not after

    def test_votes_count_for_their_rules_in_the_period_and_a_late_one_for_either_answer(self):
        unbonded = vote(governance.UNBONDED, 'GOVERNANCE_VOTER_NOT_ELIGIBLE', weight=None)
        self.assertEqual(governance.classify(unbonded), 'refused')
        landed_late = vote(governance.UNBONDED, 'GOVERNANCE_VOTER_NOT_ELIGIBLE', weight=None, landed=81)
        self.assertEqual(governance.classify(landed_late), 'refused_other')
        repeated = vote(governance.REPEATED, 'GOVERNANCE_ALREADY_VOTED', voted='yes')
        self.assertEqual(governance.classify(repeated), 'refused')
        changed = vote(governance.REPEATED, 'GOVERNANCE_ALREADY_VOTED', voted='yes',
                       effect={'what': 'vote', 'before': 'yes', 'after': 'no'})
        self.assertEqual(governance.classify(changed), 'admitted')
        self.assertEqual(governance.classify(vote(governance.REPEATED, '', voted='yes', result=result(code=0, log=''))),
                         'admitted')
        for log in governance.LATE_RULES:
            self.assertEqual(governance.classify(vote(governance.LATE, log, height=81, landed=83)), 'refused')
        self.assertEqual(governance.classify(vote(governance.LATE, 'GOVERNANCE_VOTER_NOT_ELIGIBLE', height=81,
                                                  landed=83)), 'refused_other')

    def test_a_proposal_below_the_deposit_counts_when_it_collects_exactly_its_deposit(self):
        self.assertEqual(governance.classify(below()), 'refused')
        self.assertEqual(governance.classify(below(amount=0)), 'refused')
        self.assertEqual(governance.classify(below(phase='voting')), 'admitted')
        self.assertEqual(governance.classify(below(deposited=3)), 'admitted')
        self.assertEqual(governance.classify(below(proposal_view=None)), 'admitted')
        self.assertEqual(governance.classify(below(udgt_change=0)), 'admitted')
        refund = [{'proposal_id': '2', 'udgt': '4'}]
        self.assertEqual(governance.classify(below(refunds=refund)), 'refused')
        self.assertEqual(governance.classify(below(refunds=refund, udgt_change=-2)), 'admitted')
        # The proposal itself must stand, and its deposit commit.
        self.assertEqual(governance.classify(below(result=result(log='GOVERNANCE_PROPOSAL_ID'))), 'refused_other')
        self.assertEqual(governance.classify(below(deposit={'code': 0, 'log': '', 'result': None})), 'transport')
        self.assertEqual(governance.classify(below(deposit={'code': 0, 'log': '',
                                                            'result': result(log='GOVERNANCE_DEPOSIT_CLOSED')})),
                         'refused_other')
        too_much = below(amount=MINIMUM)
        self.assertEqual(governance.classify(too_much), 'refused_other')

    def test_the_watches(self):
        close = {'close_height': 10, 'voting_period_blocks': 720}
        self.assertTrue(governance.close_ok({**close, 'read_height': 10, 'phase': 'finished',
                                             'outcome': 'below_minimum'}))
        self.assertTrue(governance.close_ok({**close, 'read_height': 11, 'phase': None}))
        self.assertTrue(governance.close_ok({**close, 'read_height': 730, 'phase': None}))
        # Removed so long after its close that a vote opened then would be over.
        self.assertFalse(governance.close_ok({**close, 'read_height': 731, 'phase': None}))
        self.assertFalse(governance.close_ok({**close, 'read_height': 11, 'phase': 'voting'}))
        self.assertFalse(governance.close_ok({**close, 'read_height': 11, 'phase': 'finished', 'outcome': 'rejected'}))
        self.assertFalse(governance.close_ok({**close, 'read_height': 9, 'phase': None}))
        self.assertTrue(governance.tally_ok({'before': {'votes': 1}, 'after': {'votes': 1}, 'admitted': 0}))
        self.assertFalse(governance.tally_ok({'before': {'votes': 1}, 'after': {'votes': 2}, 'admitted': 0}))
        self.assertTrue(governance.refunds_ok({'udgt_change': '6', 'refunds': [{'udgt': '2'}, {'udgt': '4'}]}))
        self.assertFalse(governance.refunds_ok({'udgt_change': '0', 'refunds': [{'udgt': '2'}]}))


# A stand-in node, through the desk's channel and CLI

def account_id(label):
    return hashlib.sha256(label.encode()).hexdigest()


def wallets():
    labels = [PROPOSER, DEPOSITOR, *PARAMETER, *REPEATERS, *LOADS]
    return {label: load.Wallet(label, 'voter', f'dyt1{label}', account_id(label), f'keys/{label}.json')
            for label in labels}


class Chain:
    """Blocks advance one per head read. A transaction goes in the next
    block; its result is code 0, or 3 and the rule, charged and its nonce
    spent; the node's checks run in its order."""

    def __init__(self, deposit_period=120_960, voting_period=120_960, accept_out_of_bounds=False,
                 open_below_minimum=False, keep_deposits=False, count_second_votes=False, drop=(), cli_errors=()):
        self.height, self.next_id = 10, 1
        self.deposit_period, self.voting_period = deposit_period, voting_period
        self.accept_out_of_bounds, self.open_below_minimum = accept_out_of_bounds, open_below_minimum
        self.keep_deposits, self.count_second_votes = keep_deposits, count_second_votes
        self.drop, self.cli_errors = set(drop), set(cli_errors)
        self.sent, self.calls = 0, 0
        self.accounts = {}
        for label in wallets():
            bonded = 0 if label.startswith('load') else BOND
            liquid = MINIMUM if label == DEPOSITOR else 1_000_000 if label == PROPOSER else 0
            self.accounts[account_id(label)] = {'liquid_udrt': 10**12, 'liquid_udgt': liquid, 'nonce': 1,
                                                'bonded_udgt': bonded}
        self.proposals, self.votes, self.weights = {}, {}, {}
        self.mempool, self.blocks = [], {}
        self.slots, self.connections = threading.BoundedSemaphore(2), 2
        self.lock = threading.Lock()

    # Blocks

    def tick(self):
        with self.lock:
            self.height += 1
            self.begin_block()
            txs, self.mempool = self.mempool, []
            self.blocks[self.height] = [(tx, self.execute(tx)) for tx in txs]

    def begin_block(self):
        h = self.height
        for ident in [i for i, p in self.proposals.items() if p.get('removed_at') == h]:
            del self.proposals[ident]
        for ident, p in self.proposals.items():
            phase = p['phase']
            if phase['phase'] == 'collecting' and phase['close_height'] == h:
                if p['deposited'] < MINIMUM and not self.open_below_minimum:
                    self.finish(ident, 'below_minimum')
                else:
                    snapshot = h - 1
                    for owner, a in self.accounts.items():
                        if a['bonded_udgt']:
                            self.weights[(snapshot, owner)] = a['bonded_udgt']
                    p['phase'] = {'phase': 'voting', 'snapshot_height': str(snapshot),
                                  'end_height': str(h + self.voting_period - 1),
                                  'tally': {'yes': '0', 'no': '0', 'no_with_veto': '0', 'abstain': '0', 'votes': 0}}
            elif phase['phase'] == 'voting' and int(phase['end_height']) + 1 == h:
                self.finish(ident, 'rejected')

    def finish(self, ident, outcome):
        p = self.proposals[ident]
        if not self.keep_deposits:
            for owner, amount in p['deposits'].items():
                self.accounts[owner]['liquid_udgt'] += amount
        p['phase'] = {'phase': 'finished', 'outcome': outcome, 'failure': None, 'height': str(self.height)}
        p['removed_at'] = self.height + 1

    def execute(self, tx):
        a = self.accounts[tx['owner']]
        a['nonce'] += 1
        a['liquid_udrt'] -= CHARGE
        rule = self.rule(tx)
        return {'code': 3 if rule else 0, 'log': rule or '', 'gas_used': str(GAS_USED), 'gas_wanted': str(GAS_USED)}

    def rule(self, tx):
        action, owner = tx['action'], tx['owner']
        if action['type'] == 'GovernanceProposal':
            name, value = action['change']
            bound = BOUNDS[name]
            if not bound['min'] <= value <= bound['max'] and not self.accept_out_of_bounds:
                return 'GOVERNANCE_PARAMETER_OUT_OF_BOUNDS'
            if not self.accounts[owner]['bonded_udgt']:
                return 'GOVERNANCE_PROPOSER_NOT_BONDED'
            if action['proposal_id'] != self.next_id:
                return 'GOVERNANCE_PROPOSAL_ID'
            self.proposals[action['proposal_id']] = {
                'proposer': owner, 'admitted_height': self.height, 'deposits': {}, 'deposited': 0,
                'phase': {'phase': 'collecting', 'close_height': self.height + self.deposit_period}}
            self.next_id += 1
            return None
        p = self.proposals.get(action['proposal_id'])
        if p is None:
            return 'GOVERNANCE_PROPOSAL_UNKNOWN'
        if action['type'] == 'GovernanceDeposit':
            if p['phase']['phase'] != 'collecting':
                return 'GOVERNANCE_DEPOSIT_CLOSED'
            p['deposits'][owner] = p['deposits'].get(owner, 0) + action['amount']
            p['deposited'] += action['amount']
            self.accounts[owner]['liquid_udgt'] -= action['amount']
            return None
        phase = p['phase']
        if phase['phase'] != 'voting' or self.height > int(phase['end_height']):
            return 'GOVERNANCE_VOTING_CLOSED'
        if (int(phase['snapshot_height']), owner) not in self.weights:
            return 'GOVERNANCE_VOTER_NOT_ELIGIBLE'
        key = (action['proposal_id'], owner)
        if key in self.votes and not self.count_second_votes:
            return 'GOVERNANCE_ALREADY_VOTED'
        if key not in self.votes:
            phase['tally']['votes'] += 1
        self.votes[key] = action['choice']
        return None

    # The CLI's online path

    def cli(self, label, args):
        with self.lock:
            return self.write(label, args)

    def write(self, label, args):
        self.sent += 1
        if self.sent in self.cli_errors:
            return 1, '', 'Error: the channel: connection reset'
        owner = account_id(label)
        a = self.accounts[owner]
        _, what, *rest = args
        flags = dict(zip(rest[::2], rest[1::2]))
        if what == 'propose':
            name = 'max_active' if '--max-active' in flags else 'min_self_bond'
            value = int(flags.get('--max-active', flags.get('--min-self-bond-udgt')))
            action = {'type': 'GovernanceProposal', 'proposal_id': self.next_id, 'change': (name, value)}
        elif what == 'deposit':
            whole, _, fraction = flags['--amount'].partition('.')
            amount = int(whole) * load.UNIT + int(fraction.ljust(6, '0'))
            if amount <= 0:
                return 1, '', 'Error: amount must be positive'
            action = {'type': 'GovernanceDeposit', 'proposal_id': int(flags['--proposal-id']), 'amount': amount}
        else:
            action = {'type': 'GovernanceVote', 'proposal_id': int(flags['--proposal-id']),
                      'choice': flags['--choice'].replace('-', '_')}
        if any(tx['owner'] == owner for tx in self.mempool):
            return 1, '', 'Error: CheckTx rejected the transaction (1): Admission reservation rejected: Counter is already reserved'
        if action['type'] == 'GovernanceDeposit' and action['amount'] > a['liquid_udgt']:
            return 1, '', 'Error: CheckTx rejected the transaction (1): Governance deposit exceeds eligible uDGT'
        tx = {'owner': owner, 'nonce': a['nonce'], 'action': action, 'n': self.sent}
        raw = json.dumps(tx, sort_keys=True).encode()
        tx['raw'] = raw
        if self.sent not in self.drop:
            self.mempool.append(tx)
        body = {'spending_nonce': str(a['nonce']), 'expiry_height': str(self.height + 1 + 4),
                'actions': [{'type': action['type'], 'proposal_id': str(action['proposal_id'])}]}
        report = {'transaction_id': 'ab' * 32, 'body': body,
                  'check_tx': {'code': 0, 'log': '', 'engine_hash': hashlib.sha256(raw).hexdigest().upper()}}
        return 0, 'signing\n' + json.dumps(report) + '\n', ''

    # JSON-RPC

    def call(self, label, requests):
        self.calls += 1
        return [self.answer(method, params) for method, params in requests]

    def one(self, label, method, params):
        return self.call(label, [(method, params)])[0]

    def answer(self, method, params):
        if method == 'abci_info':
            self.tick()
            return {'response': {'last_block_height': str(self.height)}}
        if method == 'block':
            txs = self.blocks.get(int(params['height']), [])
            return {'block': {'data': {'txs': [base64.b64encode(tx['raw']).decode() for tx, _ in txs]}}}
        if method == 'block_results':
            return {'txs_results': [r for _, r in self.blocks[int(params['height'])]] or None}
        if method != 'abci_query':
            raise AssertionError(method)
        value = self.query(params['path'])
        return {'response': {'code': 0, 'value': base64.b64encode(json.dumps(value).encode()).decode()}}

    def query(self, path):
        context = {'height': str(self.height)}
        parts = path.strip('/').split('/')
        if path.startswith('/account/'):
            a = self.accounts[account_id(parts[1][len('dyt1'):])]
            bonds = [{'validator_id': 'validator-1', 'amount_udgt': str(a['bonded_udgt'])}] if a['bonded_udgt'] else []
            return {'context': context, 'liquid_udgt': str(a['liquid_udgt']), 'liquid_udrt': str(a['liquid_udrt']),
                    'nonce': str(a['nonce']), 'bonded_udgt': str(a['bonded_udgt']), 'bonds': bonds,
                    'pending_bond_udgt': '0', 'unbonding_udgt': '0', 'unbonds': [], 'claimable_rewards_udrt': '0'}
        if path == '/ordinary/profile_v3':
            return {'enabled': True, 'context': context, 'next_proposal_id': str(self.next_id),
                    'fee_profile': {'base': {'gas_price': GAS_PRICE, 'minimum_gas': MINIMUM_GAS}}}
        if path.startswith('/governance/proposal/'):
            p = self.proposals.get(int(parts[2]))
            return p and {'context': context, 'proposal_id': parts[2], 'proposer': p['proposer'],
                          'admitted_height': str(p['admitted_height']), 'deposited_udgt': str(p['deposited']),
                          'depositors': len(p['deposits']),
                          'phase': {k: (str(v) if k == 'close_height' else v) for k, v in p['phase'].items()}}
        if path.startswith('/governance/vote/'):
            ident = int(parts[2])
            if ident not in self.proposals:
                return None
            return {'context': context, 'choice': self.votes.get((ident, parts[3]))}
        if path.startswith('/state/proof/'):
            key = bytes.fromhex(parts[2]).decode()
            _, _, _, snapshot, owner = key.split(':')
            weight = self.weights.get((int(snapshot), owner))
            return {'value': None if weight is None else base64.b64encode(weight.to_bytes(16, 'little')).decode()}
        raise AssertionError(path)


def desk(chain, deposit_period=120_960, voting_period=120_960):
    settings = load.Settings(expiry_blocks=4, poll_interval=0, block_time=1)
    # A wait is a block: the stand-in's blocks come with the desk's reads.
    d = governance.Desk(chain, 'dytallix', {}, settings, config(deposit_period, voting_period), ROLES, wallets(),
                        show=lambda *_: None, sleep=lambda *_: chain.tick())
    d.cli = chain.cli
    d.fees()
    return d


def campaign(attempts=3, kinds=governance.CI_KINDS, deposit_period=120_960, voting_period=120_960, **knobs):
    chain = Chain(deposit_period, voting_period, **knobs)
    run = governance.Campaign(desk(chain, deposit_period, voting_period), attempts, show=lambda *_: None)
    rows = run.run(list(kinds))
    record = governance.build_record('dytallix-staging-1', GENESIS, ENGINE, rows, attempts,
                                     governance.materials_of(run.desk, kinds), '2026-11-05T00:00:00Z',
                                     '2026-11-05T01:00:00Z')
    return chain, rows, record


def status(record):
    return {kind: t['status'] for kind, t in record['types'].items()}


ALL = list(governance.REASONS)
# Short periods: every proposal closes in the run and a vote opens.
REHEARSAL = {'deposit_period': 25, 'voting_period': 70}
# Shorter still: the proposals close while the proposals below the deposit run.
SHORT = {'deposit_period': 8, 'voting_period': 70}


class Campaigns(unittest.TestCase):
    def test_ci_runs_the_two_types_that_need_no_vote(self):
        chain, rows, record = campaign()
        self.assertEqual(record['status'], governance.PASS, record['checks'])
        self.assertEqual(status(record), {governance.BELOW: governance.PASS, governance.OUT_OF_BOUNDS: governance.PASS,
                                          **{kind: 'not_run' for kind in governance.VOTES}})
        self.assertEqual(record['types'][governance.OUT_OF_BOUNDS]['first_log'],
                         'GOVERNANCE_PARAMETER_OUT_OF_BOUNDS')
        self.assertEqual(record['closes_read'], 0)
        below = [r for r in rows if r['type'] == governance.BELOW]
        self.assertEqual([r['fault']['deposit_udgt'] for r in below], ['0', '2', '0'])
        self.assertEqual([r['proposal_view']['deposited_udgt'] for r in below], ['0', '2', '0'])
        # The two controls and the three proposals each took the next ID.
        self.assertEqual(chain.next_id, 6)
        bounds = [r for r in rows if r['type'] == governance.OUT_OF_BOUNDS]
        self.assertEqual([(r['fault']['parameter'], r['fault']['value']) for r in bounds],
                         [('max_active', '33'), ('max_active', '3'), ('min_self_bond', '1000000000001')])
        self.assertEqual({r['wallet'] for r in bounds}, set(PARAMETER))
        for row in rows:
            self.assertNotIn('dyt1', json.dumps(row))

    def test_a_rehearsal_runs_every_type_and_reads_each_close_and_refund(self):
        chain, rows, record = campaign(kinds=ALL, **REHEARSAL)
        self.assertEqual(record['status'], governance.PASS, (record['checks'], status(record)))
        self.assertEqual(set(status(record).values()), {governance.PASS})
        self.assertEqual(record['types'][governance.UNBONDED]['first_log'], 'GOVERNANCE_VOTER_NOT_ELIGIBLE')
        self.assertEqual(record['types'][governance.REPEATED]['first_log'], 'GOVERNANCE_ALREADY_VOTED')
        self.assertIn(record['types'][governance.LATE]['first_log'], governance.LATE_RULES)
        closes = [r for r in rows if r['type'] == 'close']
        self.assertEqual(len(closes), 5)  # the two controls and the three proposals
        # Read as they pass, during the wait for the vote too.
        self.assertTrue(all(r['read_height'] - r['close_height'] <= 3 for r in closes),
                        [(r['close_height'], r['read_height']) for r in closes])
        refunds = [r for r in rows if r['type'] == 'refunds']
        self.assertEqual(sum(int(x['udgt']) for r in refunds for x in r['refunds'])
                         + sum(int(x['udgt']) for r in rows if r['type'] == governance.BELOW
                               for x in r['refunds']), 2)
        self.assertTrue([r for r in rows if r['type'] == 'tally'])
        self.assertEqual(record['materials']['repeated_voters'], REPEATERS)

    def test_a_node_that_accepts_an_out_of_bounds_change_fails_and_the_type_stops(self):
        _, rows, record = campaign(accept_out_of_bounds=True)
        found = record['types'][governance.OUT_OF_BOUNDS]
        self.assertEqual(found['status'], governance.FAIL)
        self.assertEqual(found['counts']['admitted'], 1)  # the round's other attempt took a stale ID
        self.assertEqual(record['status'], governance.FAIL)

    def test_a_proposal_below_the_minimum_that_reaches_its_vote_is_a_success(self):
        _, rows, record = campaign(kinds=[governance.BELOW], **SHORT, open_below_minimum=True)
        self.assertFalse(record['checks']['closes'])
        self.assertEqual(record['status'], governance.FAIL)

    def test_a_deposit_kept_at_its_close_fails_the_refunds(self):
        _, rows, record = campaign(kinds=[governance.BELOW], **SHORT, keep_deposits=True)
        # The missing refund shows in the read that spans its close: an
        # attempt's own (a success) or the reads between attempts.
        self.assertTrue(not record['checks']['refunds']
                        or record['types'][governance.BELOW]['counts']['admitted'])
        self.assertEqual(record['status'], governance.FAIL)

    def test_short_periods_still_pass_with_every_close_read(self):
        _, rows, record = campaign(kinds=[governance.BELOW], **SHORT)
        self.assertEqual(record['status'], governance.PASS, record['checks'])
        self.assertGreater(record['closes_read'], 0)

    def test_a_second_vote_that_counts_is_a_success(self):
        _, rows, record = campaign(kinds=[governance.REPEATED], **REHEARSAL, count_second_votes=True)
        self.assertEqual(record['types'][governance.REPEATED]['counts']['admitted'], 2)
        self.assertEqual(record['status'], governance.FAIL)

    def test_an_attempt_that_never_committed_is_made_again(self):
        # The second write is one of the first two out-of-bounds attempts
        # (after the control's proposal): dropped, it expires and is made
        # again.
        _, rows, record = campaign(drop={2})
        found = record['types'][governance.OUT_OF_BOUNDS]
        self.assertEqual(found['counts']['transport'], 1)
        self.assertEqual(found['counts']['refused'], 3)
        self.assertEqual(record['status'], governance.PASS)

    def test_a_cli_failure_before_the_node_answered_is_not_a_refusal(self):
        _, rows, record = campaign(cli_errors={2})
        transport = [r for r in rows if r.get('error')]
        self.assertEqual(len(transport), 1)
        self.assertEqual(transport[0]['outcome'], 'transport')
        self.assertEqual(record['status'], governance.PASS)

    def test_a_late_vote_never_sent_before_the_end(self):
        _, rows, _ = campaign(kinds=[governance.LATE], **REHEARSAL)
        late = [r for r in rows if r['type'] == governance.LATE]
        self.assertTrue(all(r['result']['height'] > r['fault']['end_height'] for r in late))
        self.assertTrue(all(r['fault']['weight'] is not None for r in late))

    def test_verify_recomputes_every_outcome_from_the_rows(self):
        _, rows, record = campaign(kinds=ALL, **REHEARSAL)
        raw = governance.rows_bytes(rows)
        self.assertEqual(governance.verify(record, raw, GENESIS, ENGINE), governance.PASS)
        with self.assertRaises(governance.Failed):
            governance.verify(record, raw, b'{"other": 1}', ENGINE)
        tampered = copy.deepcopy(rows)
        row = next(r for r in tampered if r['type'] == governance.OUT_OF_BOUNDS)
        row['result']['log'] = 'GOVERNANCE_PROPOSAL_ID'
        with self.assertRaises(governance.Failed):
            governance.verify(record, governance.rows_bytes(tampered), GENESIS, ENGINE)
        with self.assertRaises(governance.Failed):
            governance.build_record('c', GENESIS, ENGINE, tampered, 3, record['materials'], 's', 'f')

    def test_the_launch_page_counts_the_familys_fewest_type(self):
        _, _, record = campaign()
        attempts, successes = results.attack_kpis('governance', [record])
        self.assertEqual((attempts['value'], attempts['status']), (0, 'in_progress'))  # the votes are not run
        self.assertEqual((successes['value'], successes['status']), (0, 'in_progress'))
        _, _, full = campaign(kinds=ALL, **REHEARSAL)
        attempts, _ = results.attack_kpis('governance', [full])
        self.assertEqual(attempts['value'], 3)


class DeskChannel(unittest.TestCase):
    def test_a_refusal_at_admission_is_the_nodes_answer_and_a_cli_error_is_not(self):
        d = desk(Chain())
        d.cli = lambda label, args: (1, '', 'Error: CheckTx rejected the transaction (1): Governance deposit is zero')
        self.assertEqual(d.answer(PROPOSER, ()), {'code': 1, 'log': 'Governance deposit is zero'})
        d.cli = lambda label, args: (1, '', 'Error: amount must be positive')
        self.assertEqual(d.answer(PROPOSER, ()), {'error': 'Error: amount must be positive'})

    def test_every_out_of_bounds_value_is_outside_its_bound_and_above_zero(self):
        d = desk(Chain())
        before = {'bonded_udgt': str(BOND)}
        context = {'next_proposal_id': '1'}
        values = {}
        for k in range(60):
            args, fault = d.plan(governance.OUT_OF_BOUNDS, k, PARAMETER[0], before, context)
            low, high = fault['bounds']
            self.assertFalse(low <= int(fault['value']) <= high, fault)
            self.assertGreater(int(fault['value']), 0)
            self.assertEqual(args[3], int(fault['value']))
            values.setdefault((fault['parameter'], int(fault['value']) > high), set()).add(fault['value'])
        self.assertEqual(sorted(values[('max_active', False)]), ['1', '2', '3'])
        self.assertEqual(len(values[('min_self_bond', True)]), 15)

    def test_amounts_and_keys(self):
        self.assertEqual(governance.dgt(2), '0.000002')
        self.assertEqual(governance.dgt(10_000_000_000), '10000.000000')
        key = bytes.fromhex(governance.weight_key(39, 'ab' * 32)).decode()
        self.assertEqual(key, 'governance:v2:weight:00000000000000000039:' + 'ab' * 32)

    def test_results_come_from_the_block_that_holds_them_or_the_expiry_passes(self):
        chain = Chain(drop={2})
        chain.connections = 1  # one write at a time, in order
        d = desk(chain)
        answers = d.submit([(PARAMETER[0], ('governance', 'propose', '--max-active', 40), None),
                            (PARAMETER[1], ('governance', 'propose', '--max-active', 41), None)])
        found = d.results(answers)
        first, second = (found[a['hash']] for a in answers)
        self.assertEqual((first['code'], first['log'], first['charge']), (3, 'GOVERNANCE_PARAMETER_OUT_OF_BOUNDS',
                                                                          str(CHARGE)))
        self.assertIsNone(second)


if __name__ == '__main__':
    unittest.main()
