"""The load generator (simulation plan, T3) without a chain: the mix, the
actions, the signer's inputs, the JSON-RPC client against a stand-in
gateway, the evidence and its verification, and a whole run (funding, first
spends, the paced window and governance) against a simulated chain. The
staging network runs it for real in the Host install workflow."""
import base64
import hashlib
import json
import random
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import load
import reconcile

CHAIN = 'dytallix-staging-1'
CONTEXT = {'chain_id': CHAIN, 'genesis_digest': 'ab' * 32, 'height': '10', 'app_hash': '11' * 32}


def wallet(label='w', role='load', drt=100 * load.UNIT, dgt=100 * load.UNIT):
    w = load.Wallet(label, role, f'tdytallix1{label}', hashlib.sha256(label.encode()).hexdigest())
    w.drt, w.dgt = drt, dgt
    return w


def account_view(w, nonce=0, height='10'):
    return {'version': 1, 'context': dict(CONTEXT, height=height),
            'domain': {'network': 2, 'chain_id': CHAIN, 'genesis_digest': 'ab' * 32, 'account_id': w.account_id},
            'account_id': w.account_id, 'address': w.address,
            'current_key': {'algorithm': 'mldsa65', 'public_key': [1, 2, 3]}, 'authorization_generation': '0',
            'spending_nonce': str(nonce), 'protected': False, 'profile_digest': 'cd' * 32}


class PieceTests(unittest.TestCase):
    def test_the_ledger_schema_is_the_reconciliations(self):
        self.assertEqual(load.LEDGER_SCHEMA, reconcile.LEDGER_SCHEMA)

    def test_the_mix(self):
        self.assertEqual(load.parse_mix('send_drt=3, claim=1'), {'send_drt': 3, 'claim': 1})
        for bad in ('send=1', 'send_drt=x', 'send_drt=0'):
            with self.assertRaises(load.Failed):
                load.parse_mix(bad)

    def test_a_kind_needs_what_it_spends_after_the_fee_cap(self):
        cap = 2 * load.UNIT
        w = wallet(drt=cap + load.UNIT, dgt=5 * load.UNIT)
        self.assertTrue(load.eligible(w, 'send_drt', 100, cap, 0))
        self.assertTrue(load.eligible(w, 'send_dgt', 100, cap, 0))
        self.assertFalse(load.eligible(w, 'bond', 100, cap, 0))  # 10 DGT
        self.assertFalse(load.eligible(w, 'create', 100, cap, 0))  # no spare left
        self.assertFalse(load.eligible(w, 'claim', 100, cap, 0))  # nothing bonded
        w.bonded, w.bond_height = 10 * load.UNIT, 98
        self.assertFalse(load.eligible(w, 'unbond', 100, cap, 0))  # not yet in the set
        self.assertTrue(load.eligible(w, 'unbond', 101, cap, 0))
        poor = wallet(drt=cap - 1)
        self.assertIsNone(load.choose(random.Random(1), poor, load.parse_mix(load.DEFAULT_MIX), 100, cap, 5))
        kinds = {load.choose(random.Random(n), w, {'send_drt': 1, 'bond': 5}, 101, cap, 0) for n in range(20)}
        self.assertEqual(kinds, {'send_drt'})

    def test_the_actions_are_the_nodes_json(self):
        recipient = 'ff' * 32
        self.assertEqual(load.actions_for('send_dgt', recipient),
                         [{'type': 'Send', 'recipient': [255] * 32, 'denomination': 'udgt', 'amount': '1000000'}])
        self.assertEqual(load.actions_for('bond', validator='v1'),
                         [{'type': 'RewardBond', 'validator_id': 'v1', 'amount_udgt': '10000000'}])
        self.assertEqual(load.actions_for('unbond', validator='v1')[0]['type'], 'RewardBeginUnbond')
        self.assertEqual(load.actions_for('claim'), [{'type': 'RewardClaim'}])
        with self.assertRaises(load.Failed):
            load.actions_for('flood')

    def test_the_signer_signs_at_the_tracked_nonce_against_the_profiles_context(self):
        w = wallet()
        profile = {'version': 1, 'enabled': True, 'context': dict(CONTEXT, height='42'), 'config': {}}
        account, context = load.signing_inputs(profile, account_view(w, nonce=3, height='40'), 7)
        self.assertEqual((account['spending_nonce'], account['context']), ('7', profile['context']))
        self.assertEqual(context['committed'], profile['context'])
        self.assertEqual(context['spending_nonce'], '7')
        self.assertEqual(context['domain']['account_id'], list(bytes.fromhex(w.account_id)))
        self.assertEqual(context['profile_digest'], [0xcd] * 32)
        self.assertEqual(set(context), {'domain', 'current_key', 'authorization_generation', 'spending_nonce',
                                        'committed', 'profile_digest', 'protected'})

    def test_percentiles_and_header_times(self):
        self.assertEqual(load.percentile([5, 1, 3, 2, 4], 0.5), 3)
        self.assertEqual(load.percentile(list(range(1, 101)), 0.99), 99)
        self.assertEqual(load.distribution([]), {'n': 0})
        self.assertAlmostEqual(load.header_seconds('2026-10-12T00:00:01.5Z') - load.header_seconds('2026-10-12T00:00:00Z'), 1.5)
        self.assertAlmostEqual(load.header_seconds('2026-10-12T00:00:00.000000001Z') % 1, 1e-9, places=6)
        with self.assertRaises(load.Failed):
            load.header_seconds('yesterday')

    def test_a_governance_charge_is_the_fall_in_liquid_drt_around_its_nonce(self):
        before = {'nonce': '1', 'liquid_udrt': '50000000', 'context': {'height': '20'}}
        after = {'nonce': '2', 'liquid_udrt': '49000000', 'context': {'height': '22'}}
        self.assertEqual(load.governance_charge(before, after, 1)['charge_udrt'], '1000000')
        with self.assertRaises(load.Failed):
            load.governance_charge(before, dict(after, nonce='3'), 1)  # two spends: ambiguous
        with self.assertRaises(load.Failed):
            load.governance_charge(before, dict(after, liquid_udrt='50000000'), 1)

    def test_a_receipt_must_be_the_rows_transaction(self):
        w = wallet()
        row = dict(load.new_row('run', 'create', w), id='aa' * 32, nonce=4, created_accounts=1)
        receipt = {'transaction_id': 'aa' * 32, 'actor': w.account_id, 'nonce_before': '4', 'nonce_after': '5',
                   'block_height': '12', 'outcome': 'application_failure', 'charge': '1000000',
                   'context': {'height': '12'}}
        self.assertEqual(load.apply_receipt(row, receipt, w.account_id), 5)
        self.assertEqual((row['status'], row['block_height'], row['created_accounts']), ('committed', 12, 0))
        with self.assertRaises(load.Failed):
            load.apply_receipt(dict(row, nonce=3), receipt, w.account_id)


class GatewayStandIn(BaseHTTPRequestHandler):
    """Answers JSON-RPC as the gateway relays the node's replies."""
    replies = {}

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path != '/rpc' or self.headers['Content-Type'] != 'application/json':
            self.send_response(415)
            self.end_headers()
            return
        if isinstance(body, dict) and body['method'] == 'fail':
            self.send_response(502)
            self.end_headers()
            self.wfile.write(b'{"error":"the endpoint closed the channel"}')
            return
        requests = body if isinstance(body, list) else [body]
        out = []
        for r in requests:
            reply = self.replies.get(r['method'])
            out.append({'jsonrpc': '2.0', 'id': r['id'], **(reply(r['params']) if reply else
                                                            {'error': {'code': -32601, 'message': 'not found'}})})
        raw = json.dumps(out if isinstance(body, list) else out[0]).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def log_message(self, *args):
        pass


class RpcTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = ThreadingHTTPServer(('127.0.0.1', 0), GatewayStandIn)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.url = f'http://127.0.0.1:{cls.server.server_address[1]}'

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()

    def setUp(self):
        value = base64.b64encode(json.dumps({'height': 9}).encode()).decode()
        GatewayStandIn.replies = {
            'abci_info': lambda p: {'result': {'response': {'last_block_height': '9'}}},
            'abci_query': lambda p: {'result': {'response': {'code': 0, 'log': '', 'height': '9', 'value':
                                     value if p['path'] == '/status' else base64.b64encode(b'null').decode()}}},
            'broadcast_tx_sync': lambda p: (
                {'result': {'code': 0, 'log': '', 'codespace': '', 'data': '',
                            'hash': hashlib.sha256(base64.b64decode(p['tx'])).hexdigest().upper()}}
                if base64.b64decode(p['tx']) != b'bad' else
                {'result': {'code': 7, 'log': 'Ordinary spending nonce is stale', 'codespace': 'x', 'data': '',
                            'hash': 'AA' * 32}})}

    def test_a_batch_answers_each_request(self):
        rpc = load.Rpc(self.url)
        results = rpc.call('t', [('abci_info', {}), load.query_request('/status'), load.query_request('/x'),
                                 ('nope', {})])
        self.assertEqual(load.head_height(results[0]), 9)
        self.assertEqual(load.query_value(results[1]), {'height': 9})
        self.assertIsNone(load.query_value(results[2]))
        self.assertIsInstance(results[3], load.RpcError)
        self.assertEqual(load.query(rpc, 't', '/status'), {'height': 9})
        self.assertEqual(len(rpc.latency['t']), 2)
        with self.assertRaises(load.Failed):
            rpc.call('t', [('abci_info', {})] * 11)

    def test_an_unknown_outcome_is_transport(self):
        with self.assertRaises(load.Transport):
            load.Rpc(self.url).one('t', 'fail', {})
        with self.assertRaises(load.Transport):
            load.Rpc('http://127.0.0.1:9').one('t', 'abci_info', {})

    def test_a_broadcast_records_the_head_the_admission_and_the_refusal(self):
        w = wallet()
        good, bad = load.new_row('run', 'send_drt', w), load.new_row('run', 'send_drt', w)
        self.assertEqual(load.broadcast(load.Rpc(self.url), [(good, b'tx'), (bad, b'bad')]), 9)
        self.assertEqual((good['status'], good['submit_height'], good['engine_hash']),
                         ('admitted', 9, hashlib.sha256(b'tx').hexdigest()))
        self.assertEqual((bad['status'], bad['code']), ('refused', 7))
        self.assertIn('stale', bad['log'])
        row = load.new_row('run', 'send_drt', w)
        with self.assertRaises(load.Transport):
            load.broadcast(load.Rpc('http://127.0.0.1:9'), [(row, b'tx')])
        self.assertEqual(row['status'], 'uncertain')


def tx_row(phase, kind, status='committed', block=12, submit=10, charge='1000000', created=0, outcome='success',
           path='offline', ms=20.0):
    return {'phase': phase, 'kind': kind, 'wallet': 'w', 'path': path, 'nonce': 1, 'id': 'ab' * 32,
            'status': status, 'block_height': block if status == 'committed' else None, 'submit_height': submit,
            'outcome': outcome if status == 'committed' else None, 'charge_udrt': charge if status == 'committed' else None,
            'created_accounts': created, 'broadcast_ms': ms, 'log': None}


def supply(burned, height):
    return {'height': height, 'app_hash': 'x', 'supply': {'genesis': '1000', 'emitted': '0', 'burned': str(burned),
                                                          'total': str(1000 - burned)},
            'dgt_supply': {'issued': str(load.DGT_TOTAL)}}


SETTINGS = {'min_within_2': 0.99, 'max_refused': 0, 'governance': False}
TIMES = {str(h): f'2026-10-12T00:00:{h * 4 % 60:02d}Z' for h in range(10, 15)}


class EvidenceTests(unittest.TestCase):
    def rows(self):
        return [tx_row('setup', 'fund', created=8, block=11), tx_row('run', 'send_drt', block=11),
                tx_row('run', 'create', block=12, created=1), tx_row('run', 'claim', block=13, submit=11)]

    def record(self, rows=None, after_burned=None):
        rows = self.rows() if rows is None else rows
        raw = load.rows_jsonl(rows)
        ledger = load.ledger(rows, 10, 10)
        burned = after_burned if after_burned is not None else 4000000 + 9 * 10
        return load.evidence(CHAIN, SETTINGS, {}, raw, ledger, supply(0, 10), supply(burned, 16), TIMES, {}, {},
                             'a', 'b'), raw

    def test_the_ledger_is_the_reconciliations(self):
        ledger = load.ledger(self.rows() + [tx_row('run', 'send_drt', status='refused')], 10, 10000000)
        self.assertEqual(ledger['schema'], reconcile.LEDGER_SCHEMA)
        self.assertEqual(len(ledger['entries']), 4)
        self.assertEqual(sum(e['created_accounts'] for e in ledger['entries']), 9)
        self.assertEqual(ledger['account_creation_fee_udrt'], '10000000')

    def test_a_clean_run_passes_and_verifies(self):
        record, raw = self.record()
        self.assertEqual(record['status'], load.PASS, record['checks'])
        s = record['summary']
        self.assertEqual(s['inclusion']['histogram'], {'1': 1, '2': 2})
        self.assertEqual(s['throughput']['per_block_max'], 1)
        self.assertEqual(s['block_interval_s']['p50'], 4.0)
        self.assertEqual(load.verify(json.loads(json.dumps(record)), raw), load.PASS)

    def test_failures(self):
        rows = [tx_row('run', 'send_drt', block=11)] * 3
        cases = {'inclusion_within_2_blocks': rows + [tx_row('run', 'send_drt', block=14)],
                 'nothing_lost': rows + [tx_row('run', 'send_drt', status='lost')],
                 'refusals': rows + [tx_row('run', 'send_drt', status='refused')]}
        for check, case in cases.items():
            raw = load.rows_jsonl(case)
            ledger = load.ledger(case, 10, 10)
            charged = sum(int(e['charge_udrt']) for e in ledger['entries'])
            record = load.evidence(CHAIN, SETTINGS, {}, raw, ledger, supply(0, 10), supply(charged, 16), TIMES, {},
                                   {}, 'a', 'b')
            statuses = {c['check']: c['status'] for c in record['checks']}
            self.assertEqual(statuses[check], load.FAIL, check)
            self.assertEqual(record['status'], load.FAIL)
        record, _ = self.record(after_burned=1)
        self.assertEqual({c['check']: c['status'] for c in record['checks']}['fees_and_burns'], load.FAIL)

    def test_tampering_is_found(self):
        record, raw = self.record()
        forged = json.loads(json.dumps(record))
        forged['summary']['throughput']['per_block_max'] = 99
        with self.assertRaises(load.Failed):
            load.verify(forged, raw)
        with self.assertRaises(load.Failed):
            load.verify(record, raw.replace(b'"claim"', b'"bond"'))
        resealed = dict(forged)
        resealed.pop('sha256')
        resealed['sha256'] = load.sha256(load.canonical(resealed))
        with self.assertRaises(load.Failed):
            load.verify(resealed, raw)  # the summary must follow from the rows


# A simulated chain behind a stand-in RPC

class Chain:
    """Blocks every `block_time` seconds; the mempool admits only an
    account's committed nonce, once; each block commits it all with a
    receipt, burning the charge and each created account's fee."""
    CHARGE, FEE = 1_000_000, 10_000_000

    def __init__(self, block_time, accounts, drop=()):
        self.block_time, self.started = block_time, time.monotonic()
        self.accounts = {a: {'nonce': 0, 'liquid': 10**12} for a in accounts}
        self.mempool, self.receipts, self.times = [], {}, {}
        self.burned, self.lock, self.drop = 0, threading.Lock(), set(drop)
        self.proposals, self.next_proposal, self.votes = {}, 1, {}
        self.committed_height = 0

    def height(self):
        with self.lock:
            target = int((time.monotonic() - self.started) / self.block_time) + 5
            while self.committed_height < target:
                self.committed_height += 1
                self.commit(self.committed_height)
            return self.committed_height

    def commit(self, height):
        self.times[height] = f'2026-10-12T00:{height * 4 // 60 % 60:02d}:{height * 4 % 60:02d}Z'
        for tx in self.mempool:
            if tx['id'] in self.drop:
                continue
            account = self.accounts[tx['actor']]
            created = 0
            for action in tx.get('actions', []):
                if action['type'] == 'Send':
                    recipient = bytes(action['recipient']).hex()
                    if recipient not in self.accounts:
                        self.accounts[recipient] = {'nonce': 0, 'liquid': 0}
                        created += 1
            self.burned += self.CHARGE + created * self.FEE
            account['liquid'] -= self.CHARGE
            self.receipts[tx['id']] = {
                'transaction_id': tx['id'], 'actor': tx['actor'], 'nonce_before': str(account['nonce']),
                'nonce_after': str(account['nonce'] + 1), 'block_height': str(height), 'outcome': 'success',
                'charge': str(self.CHARGE), 'context': {'height': str(height)}}
            account['nonce'] += 1
            if tx.get('governance'):
                tx['governance'](self)
        self.mempool = []

    def admit(self, tx):
        with self.lock:
            account = self.accounts.get(tx['actor'])
            if account is None or tx['nonce'] != account['nonce'] or any(t['actor'] == tx['actor'] for t in self.mempool):
                return 5, 'Ordinary spending nonce is stale'
            self.mempool.append(tx)
            return 0, ''

    def query(self, path, height):
        context = dict(CONTEXT, height=str(height))
        if path == '/status':
            return {'height': height, 'app_hash': '00' * 32,
                    'supply': {'genesis': str(10**15), 'emitted': '0', 'burned': str(self.burned),
                               'total': str(10**15 - self.burned)}, 'dgt_supply': {'issued': str(load.DGT_TOTAL)}}
        if path == '/ordinary/profile':
            return {'version': 1, 'enabled': True, 'context': context, 'config': {'fee_profile': {'v': 1}}}
        if path == '/ordinary/profile_v3':
            return {'version': 1, 'enabled': True, 'context': context, 'next_proposal_id': str(self.next_proposal)}
        kind, _, rest = path.lstrip('/').partition('/')
        if path.startswith('/ordinary/receipt/'):
            return self.receipts.get(path.rsplit('/', 1)[1])
        if path.startswith('/ordinary/account/'):
            account_id = path.rsplit('/', 1)[1]
            account = self.accounts.get(account_id)
            if account is None:
                return None
            view = account_view(load.Wallet('x', 'load', 'tdytallix1x', account_id), account['nonce'], str(height))
            return view
        if path.startswith('/account/'):
            account_id = ADDRESSES[path.rsplit('/', 1)[1]]
            account = self.accounts[account_id]
            return {'nonce': str(account['nonce']), 'liquid_udrt': str(account['liquid']), 'context': context,
                    'bonded_udgt': str(100 * load.UNIT)}
        if path.startswith('/governance/proposal/'):
            proposal = self.proposals.get(int(path.rsplit('/', 1)[1]))
            if proposal and getattr(self, 'opens', None) and height >= self.opens:
                proposal['phase'] = {'phase': 'voting'}
            return proposal
        if path.startswith('/governance/vote/'):
            number, voter = path.split('/')[3:5]
            return {'choice': self.votes.get((int(number), voter))}
        raise AssertionError(path)


ADDRESSES = {}


class ChainRpc:
    """The Rpc interface over a simulated chain."""

    def __init__(self, chain, connections=2):
        self.chain, self.connections = chain, connections
        self.slots = threading.BoundedSemaphore(connections)
        self.lock = threading.Lock()
        self.latency = {}

    def timed(self, label, requests):
        with self.slots:
            height = self.chain.height()
            out = []
            for method, params in requests:
                if method == 'abci_info':
                    out.append({'response': {'last_block_height': str(height)}})
                elif method == 'abci_query':
                    value = self.chain.query(params['path'], height)
                    out.append({'response': {'code': 0, 'log': '', 'height': str(height),
                                             'value': base64.b64encode(json.dumps(value).encode()).decode()}})
                elif method == 'broadcast_tx_sync':
                    raw = base64.b64decode(params['tx'])
                    code, log = self.chain.admit(json.loads(raw))
                    out.append({'code': code, 'log': log, 'hash': hashlib.sha256(raw).hexdigest().upper()})
                elif method == 'header':
                    out.append({'header': {'time': self.chain.times[int(params['height'])]}})
            return out, 1.0

    def call(self, label, requests):
        return self.timed(label, requests)[0]

    def one(self, label, method, params):
        return self.call(label, [(method, params)])[0]


class ChainSigner:
    def sign(self, profile_path, profile, account, nonce, actions, expiry, gas_limit, cap, key_file=None,
             wallet=None, memo='t3'):
        assert int(profile['context']['height']) + 1 < expiry
        tx = {'id': hashlib.sha256(f'{account["account_id"]}:{nonce}'.encode()).hexdigest(),
              'actor': account['account_id'], 'nonce': nonce, 'actions': actions}
        return tx['id'], json.dumps(tx).encode()


class SimulatedLoad(load.Load):
    def online(self, wallet, args, phase='setup', kind=None):
        """The CLI's online path, against the simulated chain."""
        row = self.record(load.new_row(phase, kind or args[0], wallet, path='online'))
        row['submit_height'] = self.tracker.head
        nonce = self.rpc.chain.accounts[wallet.account_id]['nonce']
        tx = {'id': hashlib.sha256(f'{wallet.account_id}:{nonce}'.encode()).hexdigest(), 'actor': wallet.account_id,
              'nonce': nonce}
        if args[0] == 'governance':
            action = args[1]

            def effect(chain):
                if action == 'propose':
                    chain.proposals[chain.next_proposal] = {
                        'proposer': wallet.account_id, 'deposited_udgt': '0', 'phase': {'phase': 'collecting'},
                        'due_height': str(chain.committed_height + 5), 'context': {'height': str(chain.committed_height)}}
                    chain.next_proposal += 1
                elif action == 'deposit':
                    chain.proposals[int(args[3])]['deposited_udgt'] = args[5].replace('.', '')
                    # The vote opens a few blocks later, at the deposit close.
                    chain.opens = chain.committed_height + 3
                else:
                    chain.votes[(int(args[3]), wallet.account_id)] = args[5]
            tx['governance'] = effect
            row.update(nonce=nonce, expiry_height=self.rpc.chain.height() + 21)
        code, log = self.rpc.chain.admit(tx)
        row.update(id=tx['id'], code=code, log=log, status='admitted' if code == 0 else 'refused',
                   broadcast_ms=1.0, submitted_at=time.time())
        if row['expiry_height'] is None:
            row['expiry_height'] = self.rpc.chain.height() + 1 + self.s.expiry_blocks
        return row


class WholeRunTests(unittest.TestCase):
    def run_load(self, drop=(), governance=True, rate=60.0, vote=False):
        fleet = {'_dir': '.', 'chain_id': CHAIN, 'wallets': []}
        for label, role in load.fleet_labels(6, 1, 4):
            w = wallet(label, role)
            ADDRESSES[w.address] = w.account_id
            fleet['wallets'].append({'label': label, 'role': role, 'address': w.address, 'account_id': w.account_id,
                                     'key_file': f'keys/{label}.json'})
        funder = wallet('ecosystem-growth', 'funder')
        ADDRESSES[funder.address] = funder.account_id
        chain = Chain(0.05, [funder.account_id], drop=drop)
        config = {'ordinary': {'fee_profile': {'account_creation_fee_udrt': str(Chain.FEE), 'gas_price': '10'}},
                  'lifecycle': {'max_active': 16}, 'governance': {'deposit': {'minimum_deposit_udgt': 10**10}}}
        settings = load.Settings(rate=rate, duration=1.0, block_time=0.05, poll_interval=0.02, governance=governance, max_refused=0,
                                 governance_vote=vote, fund_drt=100 * load.UNIT, fund_dgt=100 * load.UNIT)
        with tempfile.TemporaryDirectory() as out:
            run = SimulatedLoad(ChainRpc(chain), ChainSigner(), {}, 'dytallix', fleet, out, settings,
                                {'label': funder.label, 'address': funder.address, 'account_id': funder.account_id},
                                'validator-1', config, show=lambda m: None)
            before = load.status_reading(run.status('s'))
            run.fund()
            run.first_spends()
            thread = threading.Thread(target=run.run_governance) if governance else None
            if thread:
                thread.start()
            window = run.window()
            if thread:
                thread.join(10)
            while chain.height() < max(r['block_height'] or 0 for r in run.rows) + 2:
                time.sleep(0.05)
            after = load.status_reading(run.status('s'))
            raw = load.rows_jsonl(run.rows)
            ledger = load.ledger(run.rows, before['height'], Chain.FEE)
            times = {str(h): t for h, t in run.block_times(before['height'] + 1, chain.height()).items()}
            settings_record = {'min_within_2': 0.99, 'max_refused': 0, 'governance': governance,
                               'governance_vote': vote}
            record = load.evidence(CHAIN, settings_record, {}, raw, ledger, before, after, times, run.governance,
                                   window, 'a', 'b')
            return run, record, raw

    def test_a_whole_run_commits_everything_and_reconciles(self):
        run, record, raw = self.run_load()
        s = record['summary']
        self.assertEqual(record['status'], load.PASS, json.dumps(record['checks'], indent=1))
        self.assertEqual(s['by_phase']['setup'], 2 + 9)  # two funding transactions, nine first spends
        self.assertGreater(s['by_phase']['run'], 20)
        self.assertEqual(set(s['by_status']), {'committed'})
        self.assertEqual(s['inclusion']['histogram'].keys() - {'1', '2'}, set())
        self.assertEqual([st['kind'] for st in record['governance']['steps']],
                         ['governance_propose', 'governance_deposit'])
        self.assertEqual((record['governance']['phase'], record['governance']['votes']), ('collecting', []))
        # Each wallet's nonces were consecutive: the node admits only the next one.
        for label in {r['wallet'] for r in run.rows if r['phase'] == 'run'}:
            nonces = [r['nonce'] for r in run.rows if r['wallet'] == label and r['phase'] == 'run']
            self.assertEqual(nonces, list(range(nonces[0], nonces[0] + len(nonces))))
        created = load.created_addresses(run.rows, run.wallets)
        self.assertEqual(len(created), 9 + sum(r['kind'] == 'create' for r in run.rows if r['phase'] == 'run'))
        self.assertEqual(load.verify(json.loads(json.dumps(record)), raw), load.PASS)

    def test_with_the_vote_the_voters_wait_for_it(self):
        _, record, raw = self.run_load(vote=True, rate=20.0)
        self.assertEqual(record['status'], load.PASS, json.dumps(record['checks'], indent=1))
        self.assertEqual([st['kind'] for st in record['governance']['steps']][-1], 'governance_vote')
        self.assertEqual(record['governance']['votes'], ['yes'])
        self.assertEqual(load.verify(json.loads(json.dumps(record)), raw), load.PASS)

    def test_a_transaction_the_chain_drops_is_lost(self):
        # The first offline run transaction of the first load wallet never commits.
        first = hashlib.sha256(f'{hashlib.sha256(b"load-0001").hexdigest()}:1'.encode()).hexdigest()
        _, record, _ = self.run_load(drop={first}, governance=False, rate=20.0)
        statuses = {c['check']: c['status'] for c in record['checks']}
        self.assertEqual(statuses['nothing_lost'], load.FAIL)
        self.assertEqual(record['status'], load.FAIL)
        self.assertEqual(len(record['summary']['lost']), 1)


if __name__ == '__main__':
    unittest.main()
