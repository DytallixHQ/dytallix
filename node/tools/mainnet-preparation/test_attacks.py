"""The attack campaign (transaction family) without a chain: the mutations,
the outcome of each answer, and whole campaigns against a stand-in node that
applies the node's admission rules in order (the size, the envelope, the
domain, the key, the signature, the expiry, the nonce and the funds) to the
campaign's own bytes behind the gateway's 1 MiB request limit,
including a node that admits an attack and one that refuses for another
reason. The staging network runs it for real in the Host install workflow."""
import base64
import hashlib
import json
from pathlib import Path
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
import unittest.mock

import adversary
import attacks
import load

CHAIN = 'dytallix-staging-1'
# The stand-in profile: the gas limit times the gas price, and its action limit.
MIN_CAP, MAX_ACTIONS = 2_000_000, 16
GENESIS, ENGINE = b'{"native": 1}', b'{"engine": 1}'


def key_sig(body, key):
    seed = hashlib.sha256(body + key.encode()).digest()
    return (seed * (attacks.SIGNATURE // 32 + 1))[:attacks.SIGNATURE]


class FakeNode:
    """Blocks every `block_time` seconds; the mempool admits an account's
    committed nonce once; each block commits it all."""

    def __init__(self, accounts, block_time=0.05, admit_overspend=False, network_reason=None, refuse_all=False,
                 admit_low_cap=False):
        self.block_time, self.started = block_time, time.monotonic()
        self.accounts = accounts  # id -> {'nonce', 'balance', 'key'}
        self.mempool, self.receipts, self.lock, self.committed_height = [], {}, threading.Lock(), 0
        self.admit_overspend, self.network_reason, self.refuse_all = admit_overspend, network_reason, refuse_all
        self.admit_low_cap = admit_low_cap
        self.connections, self.slots, self.latency = 2, threading.BoundedSemaphore(2), {}

    def height(self):
        with self.lock:
            target = int((time.monotonic() - self.started) / self.block_time) + 5
            while self.committed_height < target:
                self.committed_height += 1
                for body, tid in self.mempool:
                    account = self.accounts[body['account']]
                    account['nonce'] += 1
                    account['balance'] -= body['amount']
                    self.receipts[tid] = {'transaction_id': tid, 'block_height': str(self.committed_height)}
                self.mempool = []
            return self.committed_height

    def admit(self, raw):
        height = self.height()
        with self.lock:
            if len(raw) > 262_144:
                return load.RpcError(f'Internal error: Tx too large. Max size is 262144, but got {len(raw)}')
            if self.refuse_all:
                return {'code': 9, 'log': 'invalid ordinary signature; Malformed ordinary envelope; ' + attacks.STALE}
            wire = json.loads(raw)
            try:
                envelope = base64.b64decode(wire['envelope_base64'], validate=True)
                body = json.loads(envelope[:-attacks.SIGNATURE])
                assert len(envelope) - len(json.dumps(body, sort_keys=True).encode()) == attacks.SIGNATURE
            except (ValueError, AssertionError):
                return {'code': 1, 'log': 'Malformed ordinary envelope'}
            if body.get('actions', 1) > MAX_ACTIONS:  # the decoder's count bound
                return {'code': 1, 'log': 'Malformed ordinary envelope'}
            account = self.accounts.get(body['account'])
            if account is None:  # the first-spend path
                if (body['network'], body['chain']) != (2, CHAIN):
                    return {'code': 1, 'log': self.network_reason or 'Ordinary signed domain differs from the chain domain'}
                return {'code': 1, 'log': 'Uninitialized account requires generation and nonce zero'}
            if (body['network'], body['chain']) != (2, CHAIN):
                return {'code': 1, 'log': 'Ordinary signed domain differs from account state'}
            if body['key'] != account['key']:
                return {'code': 1, 'log': 'Ordinary key or generation is stale'}
            # The meter checks the signed gas and cap before the signature.
            if body.get('cap', MIN_CAP) < MIN_CAP and not self.admit_low_cap:
                return {'code': 4, 'log': 'invalid signed gas or cap'}
            if envelope[-attacks.SIGNATURE:] != key_sig(envelope[:-attacks.SIGNATURE], account['key']):
                return {'code': 2, 'log': 'invalid ordinary signature'}
            if body['expiry'] <= height:
                return {'code': 3, 'log': 'Ordinary submission expired or exceeds configured lifetime'}
            tid = hashlib.sha256(envelope[:-attacks.SIGNATURE]).hexdigest()
            if any(t == tid for _, t in self.mempool):
                return load.RpcError(adversary.CACHE_REFUSAL)
            if any(b['account'] == body['account'] and b['nonce'] == body['nonce'] for b, _ in self.mempool):
                return {'code': 6, 'log': 'Counter is already reserved; explicit eviction is required'}
            if body['nonce'] != account['nonce']:
                return {'code': 5, 'log': attacks.STALE}
            if body['amount'] > account['balance'] and not self.admit_overspend:
                return {'code': 7, 'log': 'Insufficient current eligible funding for fee cap and actions'}
            self.mempool.append((body, tid))
            return {'code': 0, 'log': '', 'hash': hashlib.sha256(raw).hexdigest().upper()}

    def timed(self, label, requests):
        if len(json.dumps(requests)) > 1 << 20:
            raise load.Transport('HTTP 413: {"error":"request body exceeds 1 MiB"}')
        with self.slots:
            out = []
            for method, params in requests:
                if method == 'abci_info':
                    out.append({'response': {'last_block_height': str(self.height())}})
                elif method == 'broadcast_tx_sync':
                    out.append(self.admit(base64.b64decode(params['tx'])))
                else:
                    out.append(self.query(params['path']))
            return out, 1.0

    def call(self, label, requests):
        return self.timed(label, requests)[0]

    def one(self, label, method, params):
        return self.call(label, [(method, params)])[0]

    def query(self, path):
        self.height()
        if path.startswith('/ordinary/receipt/'):
            value = self.receipts.get(path.rsplit('/', 1)[1])
        else:
            account_id = path.rsplit('/', 1)[1]
            account = self.accounts[account_id]
            value = {'account_id': account_id, 'spending_nonce': str(account['nonce']),
                     'current_key': {'algorithm': 'mldsa65', 'public_key': account['key']},
                     'domain': {'network': 2, 'chain_id': CHAIN, 'account_id': account_id}}
        return {'response': {'code': 0, 'value': base64.b64encode(json.dumps(value).encode()).decode()}}


class FakeSign:
    """Signs a fake envelope: the body's JSON and a 'signature' from the key."""

    def __init__(self, node, wallets):
        self.node, self.wallets, self.expiries = node, wallets, {}

    def __call__(self, wallet, nonce, amount, variant, expiry=None):
        height = self.node.height()
        held = expiry == 'soon'
        expiry = height + 2 if held else height + 20
        key = self.node.accounts[wallet.account_id]['key']
        if variant == 'wrong_key_signature':
            key = self.node.accounts[next(w for w in self.wallets if w is not wallet).account_id]['key']
        body = {'chain': CHAIN, 'network': 2, 'account': wallet.account_id, 'key': key, 'nonce': nonce,
                'amount': amount, 'expiry': expiry, 'cap': MIN_CAP, 'actions': 1}
        if variant == 'wrong_chain':
            body['chain'] = attacks.OTHER_CHAIN
        elif variant == 'wrong_network':
            # The account's mainnet identity, which this chain has never seen.
            body['network'], body['account'] = 1, f'mainnet-{wallet.account_id}'
        raw = json.dumps(body, sort_keys=True).encode()
        envelope = raw + key_sig(raw, key)
        tx = json.dumps({'type': 'ordinary_v2', 'envelope_base64': base64.b64encode(envelope).decode()}).encode()
        if held:
            self.expiries[attacks.digest(tx)] = expiry
        return hashlib.sha256(raw).hexdigest(), tx


class FakeForge:
    """The attack client: the CLI's transport re-signed with the account's
    own key after the one change, with the client's checks."""

    def __init__(self, node, broken=False):
        self.node, self.broken = node, broken

    def __call__(self, kind, tx, wallet, k):
        envelope = base64.b64decode(json.loads(tx)['envelope_base64'])
        body = json.loads(envelope[:-attacks.SIGNATURE])
        if kind == 'fee_cap_below_minimum':
            body['cap'] = MIN_CAP - 1 - k
        else:
            body['actions'] = MAX_ACTIONS + 1
        raw = json.dumps(body, sort_keys=True).encode()
        envelope = raw + key_sig(raw, self.node.accounts[wallet.account_id]['key'])
        checks = {'variant': kind, 'k': k, 'signature_verifies': True,
                  'profile_refusal': 'not refused' if self.broken else attacks.FORGED[kind]}
        return json.dumps({'type': 'ordinary_v2', 'envelope_base64': base64.b64encode(envelope).decode()}).encode(), checks


class FakeRun:
    def __init__(self, wallets, funder):
        self.wallets = {w.label: w for w in wallets}
        self.funder = funder

    def role(self, *roles):
        return [w for w in self.wallets.values() if w.role in roles]


def campaign(attempts=6, wallets=3, forge=False, **node_options):
    loads = [load.Wallet(f'load-{n:04}', 'load', f'tdytallix1w{n}', hashlib.sha256(f'w{n}'.encode()).hexdigest())
             for n in range(1, wallets + 1)]
    funder = load.Wallet('ecosystem-growth', 'funder', 'tdytallix1f', 'ff' * 32)
    node = FakeNode({w.account_id: {'nonce': 1, 'balance': 10**12, 'key': f'key-{w.label}'} for w in loads + [funder]},
                    **node_options)
    run = FakeRun(loads, funder)
    c = attacks.Campaign(run, node, FakeSign(node, loads), attempts, show=lambda m: None, block_time=0.05,
                         forge=FakeForge(node, broken=forge == 'broken') if forge else None)
    c.nonces(loads)
    return c


class Pieces(unittest.TestCase):
    def test_each_mutation_is_new_bytes_and_stays_in_its_field(self):
        envelope = b'B' * 500 + b'S' * attacks.SIGNATURE
        tx = json.dumps({'type': 'ordinary_v2', 'envelope_base64': base64.b64encode(envelope).decode()}).encode()
        for kind in attacks.MUTATED:
            seen = {attacks.mutate(kind, tx, k) for k in range(attacks.TARGET)}
            self.assertEqual(len(seen), attacks.TARGET, kind)
        for k in (0, 1, attacks.SIGNATURE - 2, attacks.SIGNATURE, 2999):
            flipped = base64.b64decode(json.loads(attacks.mutate('invalid_signature', tx, k))['envelope_base64'])
            self.assertEqual(flipped[:500], envelope[:500], 'the body is untouched')
            self.assertEqual(len(flipped), len(envelope))
            short = base64.b64decode(json.loads(attacks.mutate('truncated_signature', tx, k))['envelope_base64'])
            self.assertEqual(short[:500], envelope[:500])
            self.assertLess(len(short), len(envelope))

    def test_an_answer_is_an_outcome(self):
        self.assertEqual(attacks.classify('expired', 3, 'Ordinary submission expired or exceeds configured lifetime'),
                         'refused')
        self.assertEqual(attacks.classify('expired', 5, attacks.STALE), 'refused_other')
        self.assertEqual(attacks.classify('expired', 0, ''), 'admitted')
        self.assertEqual(attacks.classify('expired', None, 'the gateway: reset'), 'transport')
        self.assertEqual(attacks.classify('oversized_envelope', -1, 'Internal error: Tx too large. Max size is '
                                                                    '262144, but got 300046'), 'refused')
        self.assertEqual(attacks.classify('expired', -1, 'Ordinary submission expired'), 'refused_other',
                         'only the types refused before admission may be a JSON-RPC error')
        self.assertEqual(attacks.classify('resigned_copy', -1, adversary.CACHE_REFUSAL, same_bytes=True), 'refused')
        self.assertEqual(attacks.classify('resigned_copy', 2, adversary.IDENTITY_REFUSAL, same_bytes=False), 'refused')
        with self.assertRaisesRegex(attacks.Failed, 'must say whether'):
            attacks.classify('resigned_copy', 2, 'x')
        with unittest.mock.patch.dict(attacks.REASONS, {'too_many_actions': None}):
            with self.assertRaisesRegex(attacks.Failed, 'not built'):
                attacks.classify('too_many_actions', 1, 'x')
        with self.assertRaisesRegex(attacks.Failed, 'not an approved'):
            attacks.classify('flood', 1, 'x')

    def test_the_registry_is_the_plans_list(self):
        self.assertEqual(len(attacks.REASONS), 17)
        self.assertEqual({k for k, v in attacks.REASONS.items() if v is None}, set())
        made = set(attacks.MUTATED) | set(attacks.SIGNED) | set(attacks.PENDING) | {'expired'} | set(attacks.FORGED)
        self.assertEqual(made, set(attacks.REASONS))

    def test_a_forged_attempt_counts_only_with_the_clients_checks(self):
        checks = {'variant': 'too_many_actions', 'signature_verifies': True,
                  'profile_refusal': 'action count outside profile'}
        self.assertEqual(attacks.classify('too_many_actions', 1, 'Malformed ordinary envelope', forged=checks),
                         'refused')
        for broken in (None, dict(checks, signature_verifies=False), dict(checks, variant='fee_cap_below_minimum'),
                       dict(checks, profile_refusal='noncanonical ordinary body')):
            self.assertEqual(attacks.classify('too_many_actions', 1, 'Malformed ordinary envelope', forged=broken),
                             'refused_other', broken)
        self.assertEqual(attacks.classify('too_many_actions', None, '', forged=None), 'transport')

    def test_the_forger_runs_the_attack_client_on_the_clis_transport(self):
        with tempfile.TemporaryDirectory() as scratch:
            scratch = Path(scratch)
            client = scratch / 'dytallix-attack'
            client.write_text('#!/usr/bin/env python3\n'
                              'import json, sys\n'
                              'a = dict(zip(sys.argv[2::2], sys.argv[3::2]))\n'
                              'tx = json.load(open(a["--transport"]))\n'
                              'open(a["--out"], "w").write(json.dumps(dict(tx, forged=a["--variant"])))\n'
                              'print(json.dumps({"variant": a["--variant"], "k": int(a["--k"]), '
                              '"profile": a["--profile"], "key": a["--key-file"], "command": sys.argv[1]}))\n')
            client.chmod(0o755)
            signing = SimpleNamespace(profile=lambda variant: (scratch / f'{variant}.json', None))
            wallet = SimpleNamespace(key_file=scratch / 'load-0001.json')
            forge = attacks.Forger(client, signing, scratch / 'forged')
            tx, checks = forge('too_many_actions', b'{"type": "ordinary_v2", "envelope_base64": "QUJD"}', wallet, 7)
            self.assertEqual(json.loads(tx)['forged'], 'too_many_actions')
            self.assertEqual(checks, {'variant': 'too_many_actions', 'k': 7, 'profile': str(scratch / 'valid.json'),
                                      'key': str(scratch / 'load-0001.json'), 'command': 'ordinary-forge'})
            self.assertEqual(list((scratch / 'forged').iterdir()), [], 'nothing left behind')
            client.write_text('#!/bin/sh\necho "the key file is not the transaction\'s account key" >&2; exit 1\n')
            with self.assertRaisesRegex(attacks.Failed, 'did not forge too_many_actions: the key file'):
                forge('too_many_actions', b'{}', wallet, 0)


class Campaigns(unittest.TestCase):
    def test_every_built_type_is_refused_for_its_reason_and_the_record_verifies(self):
        c = campaign(forge=True)
        rows = c.make(list(attacks.REASONS))
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 6, 'a', 'b')
        self.assertEqual(record['status'], attacks.PASS, json.dumps(record['types'], indent=1))
        for kind, t in record['types'].items():
            if attacks.built(kind):
                self.assertEqual((t['status'], t['counts']['refused']), (attacks.PASS, max(6, t['counts']['refused'])),
                                 kind)
                self.assertEqual(t['counts']['admitted'] + t['counts']['refused_other'], 0, kind)
            else:
                self.assertEqual((t['status'], t['counts']['refused']), ('not_built', 0))
        self.assertEqual(record['successes'], 0)
        self.assertEqual(record['controls']['sent'], record['controls']['committed'])
        self.assertGreaterEqual(record['originals']['committed'], 6)
        self.assertEqual(record['originals']['sent'], record['originals']['committed'])
        raw = attacks.rows_bytes(rows)
        self.assertEqual(attacks.verify(json.loads(json.dumps(record)), raw, GENESIS, ENGINE), attacks.PASS)
        self.assertTrue(all(r['same_bytes'] is True for r in rows if r['type'] == 'resigned_copy'))
        # No address in the rows.
        self.assertNotIn(b'tdytallix1', raw)

    def test_an_admitted_attack_is_a_success_and_the_campaign_goes_on(self):
        c = campaign(admit_overspend=True)
        rows = c.make(['overspend', 'stale_nonce'])
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 6, 'a', 'b')
        self.assertEqual(record['types']['overspend']['status'], attacks.FAIL)
        self.assertGreater(record['types']['overspend']['counts']['admitted'], 0)
        self.assertEqual(record['successes'], record['types']['overspend']['counts']['admitted'])
        self.assertEqual(record['types']['stale_nonce']['status'], attacks.PASS, 'the nonce was read again')
        self.assertEqual(record['status'], attacks.FAIL)
        self.assertEqual(attacks.verify(record, attacks.rows_bytes(rows), GENESIS, ENGINE), attacks.FAIL)

    def test_a_refusal_for_another_reason_fails_the_type_and_shows_the_nodes_words(self):
        c = campaign(network_reason='Unknown ordinary account network')
        rows = c.make(['wrong_network'])
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 6, 'a', 'b')
        t = record['types']['wrong_network']
        self.assertEqual((t['status'], t['counts']['refused'], t['first_log']),
                         (attacks.FAIL, 0, 'Unknown ordinary account network'))
        self.assertEqual(record['successes'], 0)

    def test_a_node_that_refuses_everything_cannot_pass(self):
        c = campaign(attempts=3, refuse_all=True)
        rows = c.make(['invalid_signature', 'stale_nonce'])
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 3, 'a', 'b')
        self.assertEqual([t['status'] for k, t in record['types'].items() if k in ('invalid_signature', 'stale_nonce')],
                         [attacks.PASS, attacks.PASS], 'each attack alone looks refused')
        self.assertEqual(record['controls']['committed'], 0)
        self.assertEqual((record['checks']['controls'], record['status']), (False, attacks.FAIL))

    def test_the_forged_types_need_the_attack_client(self):
        rows = campaign().make(list(attacks.FORGED))
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 6, 'a', 'b')
        self.assertEqual({record['types'][k]['status'] for k in attacks.FORGED}, {'not_run'})
        rows = campaign(forge='broken').make(['too_many_actions'])
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 6, 'a', 'b')
        self.assertEqual((record['types']['too_many_actions']['status'],
                          record['types']['too_many_actions']['counts']['refused_other']), (attacks.FAIL, 18),
                         'a forgery the profile does not refuse for its reason tests nothing')

    def test_a_node_that_takes_a_cap_below_the_minimum_is_a_success(self):
        rows = campaign(forge=True, admit_low_cap=True).make(['fee_cap_below_minimum', 'too_many_actions'])
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 6, 'a', 'b')
        self.assertEqual(record['types']['fee_cap_below_minimum']['status'], attacks.FAIL)
        self.assertGreater(record['successes'], 0)
        self.assertEqual(record['types']['too_many_actions']['status'], attacks.PASS)
        forged = next(r['forged'] for r in rows if r['type'] == 'too_many_actions')
        self.assertEqual(forged['profile_refusal'], 'action count outside profile')

    def test_a_tampered_record_or_rows_are_found(self):
        c = campaign(attempts=3)
        rows = c.make(['invalid_signature', 'expired'])
        record = attacks.build_record(CHAIN, GENESIS, ENGINE, rows, 3, 'a', 'b')
        raw = attacks.rows_bytes(rows)
        with self.assertRaisesRegex(attacks.Failed, 'not the ones'):
            attacks.verify(record, raw + b'{}\n', GENESIS, ENGINE)
        with self.assertRaisesRegex(attacks.Failed, 'another genesis'):
            attacks.verify(record, raw, b'{}', ENGINE)
        # A record resealed with a claim its rows do not make.
        edited = dict(record, attempts_wanted=1, successes=0)
        edited['types'] = dict(record['types'], expired=dict(record['types']['expired'],
                                                             counts=dict(record['types']['expired']['counts'], refused=99)))
        edited['sha256'] = attacks.digest(attacks.canonical({k: v for k, v in edited.items() if k != 'sha256'}))
        with self.assertRaisesRegex(attacks.Failed, 'do not follow from the rows'):
            attacks.verify(edited, raw, GENESIS, ENGINE)
        relabelled = [dict(r, outcome='refused') if r['type'] == 'invalid_signature' and r['outcome'] == 'refused'
                      else r for r in rows]
        lie = [dict(relabelled[0], log='something else')] + relabelled[1:]
        with self.assertRaisesRegex(attacks.Failed, 'does not follow'):
            attacks.build_record(CHAIN, GENESIS, ENGINE, lie, 3, 'a', 'b')


if __name__ == '__main__':
    unittest.main()
