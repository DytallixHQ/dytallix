#!/usr/bin/env python3
"""The load generator and wallet fleet (simulation plan, T3): rehearsal
wallets and workers that send a configurable mix and rate of ordinary
transactions through the public node's client channel, and measure how the
chain handles them.

  load.py fleet --cli DYTALLIX --chain-id ID --out DIR --wallets N
                [--voters V] [--spares S]
  load.py run --cli DYTALLIX --home HOME --fleet DIR --out DIR
              --application-config FILE --funder WALLET --validator ID
              [--rate TX_PER_S] [--duration S] [--mix KIND=WEIGHT,...] ...
  load.py verify --evidence FILE

fleet makes the wallets with the release's CLI as a user does (`wallet
create`, then `wallet export`): each key goes to an owner-only key file in
DIR/keys (`algorithm`, `public_key`, `private_key`), never anywhere else. The
fleet is the load wallets, the governance wallets (a proposer, a depositor
and the voters) and spare keys whose accounts the run creates.

run, with HOME holding the CLI's pinned chain (the public node's channel pin)
and the funder's keystore (its passphrase in DYTALLIX_KEYSTORE_PASSPHRASE_FILE):
  1. reads the status view (the ledger starts at its height) and starts
     `dytallix gateway serve` on a loopback port: the driver's JSON-RPC
     (broadcast_tx_sync, abci_query, abci_info, header) goes through it over
     the release's client channel, with no process per transaction;
  2. funds the fleet from the funder, sixteen transfers a transaction, which
     creates their accounts; with distributors in the fleet it fans out
     (P01, 12 October 2026): the funder pays each distributor its share's
     needs, and the distributors, primed, pay their shares in parallel;
  3. primes each wallet with its first spend through the CLI's online path
     (a load wallet sends 1 DRT; a governance wallet bonds). Funding and
     priming are the warm-up: timed in the record, outside the load
     metrics;
  4. runs the measured window: wallets are paced at --rate for --duration,
     each choosing a kind from --mix; each transaction is signed offline with
     the release CLI (`ordinary prepare`, `sign --key-file`, `transport`) at
     the wallet's locally tracked nonce, broadcast in batches with the head
     height read in the same request, and followed to its committed receipt.
     A wallet has at most one transaction in flight: the node admits only
     the committed nonce;
  5. meanwhile, with --governance, the governance wallets propose a no-op
     parameter change and deposit its minimum, through the CLI's online v3
     path; with --governance-vote the voters then wait for the vote, which
     opens when the deposit period closes (720 blocks under the rehearsal
     timings), and vote. v3 has no receipts yet, so each charge is the fall
     in the wallet's liquid DRT around its spent nonce (these wallets
     receive nothing during the run);
  6. with --sweep-from, every --sweep-interval seconds (about hourly) each
     named keystore account (the operators, the stakers) claims its rewards
     and sends its liquid DRT above --sweep-keep back to the funder, so the
     genesis allocation stays the production one;
  7. drains, reads the status view again and the block headers, and writes
     transactions.jsonl (one row per transaction), ledger.json (the
     reconciliation's ledger, dytallix.reconcile-ledger.v1) and evidence.json
     (digest-bound: SHA-256 of its canonical form and of both other files).

The client channel takes 16 connections, 2 per address, one request each:
--connections bounds every channel request this process and its CLI calls
make at once (2 for one address). A JSON-RPC batch carries up to 10 requests.

PASS needs: no transaction lost (every one submitted is committed, or was
refused at admission); the share committed within 2 blocks of submission at
least --min-within-2; median block interval at most 6 s and 99th percentile
at most 10 s; the supply rule before and after; the DRT burned equal to the
charges plus each created account's fee; with --max-refused, at most that
many refusals; with --governance, each governance step committed and
charged (and, with --governance-vote, every vote recorded). verify recomputes everything from the files.
"""
import argparse
import base64
from collections import Counter, defaultdict, deque
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import random
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import config_checks  # noqa: E402

SCHEMA = 'dytallix.load-run.v1'
FLEET_SCHEMA = 'dytallix.load-fleet.v1'
LEDGER_SCHEMA = 'dytallix.reconcile-ledger.v1'
PASS, FAIL = 'PASS', 'FAIL'
TESTNET = 2
DGT_TOTAL = 10**15
UNIT = 10**6
KINDS = ('send_drt', 'send_dgt', 'bond', 'unbond', 'claim', 'create')
DEFAULT_MIX = 'send_drt=50,send_dgt=15,bond=10,unbond=5,claim=10,create=10'
# Base units each kind moves.
AMOUNTS = {'send_drt': 1 * UNIT, 'send_dgt': 1 * UNIT, 'bond': 10 * UNIT, 'unbond': 1 * UNIT, 'create': 1 * UNIT}
# A basic transaction uses about 47,600 gas and is charged the minimum; a
# funding transaction carries sixteen transfers that create accounts.
GAS_LIMIT = 200_000
FUNDING_GAS_LIMIT = 2_000_000
FUNDING_ACTIONS = 16
MAX_BATCH = 10
# The plan's liveness criterion (median, 99th percentile of block intervals).
BLOCK_INTERVAL_P50_S, BLOCK_INTERVAL_P99_S = 6.0, 10.0


class Failed(Exception):
    pass


class Transport(Failed):
    """A request whose outcome is unknown: it may or may not have arrived."""


class RpcError(Failed):
    """A JSON-RPC error object for one request of a batch."""


def say(message):
    print(message, flush=True)


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def now_utc():
    return datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


def private_file(path, data):
    """A new owner-only file."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as file:
        file.write(data)


def percentile(values, fraction):
    """Nearest-rank percentile of a non-empty list."""
    ordered = sorted(values)
    rank = max(1, -(-len(ordered) * fraction // 1))
    return ordered[int(rank) - 1]


def distribution(values, digits=1):
    if not values:
        return {'n': 0}
    return {'n': len(values), 'p50': round(percentile(values, 0.50), digits),
            'p95': round(percentile(values, 0.95), digits), 'p99': round(percentile(values, 0.99), digits),
            'max': round(max(values), digits)}


# The fleet

def make_wallet(cli, label, keys, scratch, passphrase):
    """One ML-DSA-65 wallet made with the release's CLI in a throwaway home,
    exported to an owner-only key file. Returns its public key."""
    home = Path(tempfile.mkdtemp(prefix=f'{label}-', dir=scratch))
    env = dict(os.environ, HOME=str(home), DYTALLIX_KEYSTORE_PASSPHRASE_FILE=str(passphrase))
    try:
        for args in (('wallet', 'create', '--name', label), ('wallet', 'export', '--output', str(home / 'key.hex'))):
            result = subprocess.run([str(cli), *args], capture_output=True, text=True, env=env)
            if result.returncode != 0:
                raise Failed(f'dytallix {args[0]} {args[1]} failed: {result.stderr.strip()}')
        entry = next(e for e in json.loads((home / '.dytallix' / 'keystore.json').read_bytes())['entries']
                     if e['name'] == label)
        public = bytes(entry['public_key'])
        private = bytes.fromhex((home / 'key.hex').read_text().strip())
        private_file(keys / f'{label}.json', json.dumps(
            {'algorithm': 'mldsa65', 'public_key': list(public), 'private_key': list(private)}).encode())
        return public
    finally:
        shutil.rmtree(home, ignore_errors=True)


def fleet_labels(wallets, voters, spares, distributors=0):
    return ([(f'load-{n:04}', 'load') for n in range(1, wallets + 1)] +
            [(f'distributor-{n:02}', 'distributor') for n in range(1, distributors + 1)] +
            [('gov-proposer', 'proposer'), ('gov-depositor', 'depositor')] +
            [(f'gov-voter-{n}', 'voter') for n in range(1, voters + 1)] +
            [(f'spare-{n:04}', 'spare') for n in range(1, spares + 1)])


def make_fleet(cli, chain_id, out, wallets, voters=1, spares=0, distributors=0, workers=4, show=say):
    """The fleet's key files in out/keys (0700) and out/fleet.json, the
    public record: each wallet's label, role, address and account ID. The
    distributors fan the funding out (P01, 12 October 2026: 25 paying 40
    each for 1,000 wallets)."""
    out = Path(out)
    keys, scratch = out / 'keys', out / 'scratch'
    keys.mkdir(parents=True, mode=0o700)
    scratch.mkdir(mode=0o700)
    passphrase = scratch / 'passphrase'
    private_file(passphrase, (sha256(os.urandom(32)) + '\n').encode())
    labels = fleet_labels(wallets, voters, spares, distributors)
    show(f'== the fleet: {len(labels)} wallets made with dytallix wallet create and export')
    started = time.monotonic()
    with ThreadPoolExecutor(max_workers=workers) as pool:
        publics = list(pool.map(lambda item: make_wallet(cli, item[0], keys, scratch, passphrase), labels))
    shutil.rmtree(scratch)
    records = []
    for (label, role), public in zip(labels, publics):
        account_id, address = config_checks.origin_account(TESTNET, chain_id, public)
        records.append({'label': label, 'role': role, 'address': address, 'account_id': account_id.hex(),
                        'key_file': f'keys/{label}.json'})
    fleet = {'schema': FLEET_SCHEMA, 'chain_id': chain_id, 'network': TESTNET, 'wallets': records}
    (out / 'fleet.json').write_text(json.dumps(fleet, indent=2) + '\n')
    show(f'== {len(records)} wallets in {time.monotonic() - started:.1f} s')
    return fleet


# The gateway and its JSON-RPC

class Gateway:
    """`dytallix gateway serve` on a loopback port: the release's relay of
    JSON-RPC over its client channel to the pinned endpoint."""

    def __init__(self, cli, env, log, port=0):
        if not port:
            with socket.socket() as probe:
                probe.bind(('127.0.0.1', 0))
                port = probe.getsockname()[1]
        self.log = open(log, 'ab')
        self.process = subprocess.Popen([str(cli), 'gateway', 'serve', '--listen', f'127.0.0.1:{port}'],
                                        stdout=subprocess.PIPE, stderr=self.log, text=True, env=env)
        line = self.process.stdout.readline()
        try:
            status = json.loads(line)
        except ValueError:
            self.stop()
            raise Failed(f'the gateway did not start: {line.strip()} {Path(log).read_text(errors="replace")[-500:]}')
        if status.get('status') != 'serving':
            self.stop()
            raise Failed(f'the gateway did not start: {status}')
        self.url = status['url'].rstrip('/')
        self.endpoint = status.get('endpoint')

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(10)
            except subprocess.TimeoutExpired:
                self.process.kill()
        self.log.close()


class Rpc:
    """JSON-RPC through the gateway. Every channel request takes one of
    `connections` slots, shared with the CLI's online calls."""

    def __init__(self, url, connections=2, timeout=45, clock=time.monotonic):
        self.url, self.timeout, self.clock = f'{url}/rpc', timeout, clock
        self.slots = threading.BoundedSemaphore(connections)
        self.connections = connections
        self.latency = defaultdict(list)
        self.lock = threading.Lock()
        # The gateway is on this machine: never through a proxy.
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def post(self, body):
        request = urllib.request.Request(self.url, data=body, headers={'Content-Type': 'application/json'})
        try:
            with self.opener.open(request, timeout=self.timeout) as response:
                return response.read()
        except urllib.error.HTTPError as error:
            raise Transport(f'HTTP {error.code}: {error.read()[:300].decode(errors="replace")}')
        except (urllib.error.URLError, OSError) as error:
            raise Transport(f'the gateway: {error}')

    def call(self, label, requests):
        """[(method, params)] in one JSON-RPC request: each result, or an
        RpcError for that request. Transport when the outcome is unknown."""
        return self.timed(label, requests)[0]

    def timed(self, label, requests):
        """call, and its response time in milliseconds."""
        if not 0 < len(requests) <= MAX_BATCH:
            raise Failed(f'a batch holds 1 to {MAX_BATCH} requests')
        body = [{'jsonrpc': '2.0', 'id': n, 'method': m, 'params': p} for n, (m, p) in enumerate(requests)]
        with self.slots:
            started = self.clock()
            raw = self.post(json.dumps(body if len(body) > 1 else body[0]).encode())
            elapsed = (self.clock() - started) * 1000
        with self.lock:
            self.latency[label].append(elapsed)
        try:
            replies = json.loads(raw)
        except ValueError:
            raise Transport(f'not JSON: {raw[:200]!r}')
        if isinstance(replies, dict):
            replies = [replies]
        by_id = {r.get('id'): r for r in replies if isinstance(r, dict)}
        out = []
        for n in range(len(requests)):
            reply = by_id.get(n)
            if reply is None:
                out.append(RpcError('no response'))
            elif reply.get('error') is not None:
                error = reply['error']
                out.append(RpcError(f'{error.get("message")}: {error.get("data")}'))
            else:
                out.append(reply.get('result'))
        return out, elapsed

    def one(self, label, method, params):
        result = self.call(label, [(method, params)])[0]
        if isinstance(result, RpcError):
            raise result
        return result


def query_request(path):
    return ('abci_query', {'path': path, 'data': '', 'height': '0', 'prove': False})


def query_value(result):
    """An abci_query result's committed JSON value (None for JSON null)."""
    if isinstance(result, RpcError):
        raise result
    response = result['response']
    if int(response.get('code') or 0) != 0:
        raise Failed(f'query refused ({response.get("code")}): {response.get("log")}')
    if response.get('value') is None:
        raise Failed('query value missing')
    return json.loads(base64.b64decode(response['value']))


def head_height(result):
    if isinstance(result, RpcError):
        raise result
    return int(result['response']['last_block_height'])


def query(rpc, label, path):
    return query_value(rpc.call(label, [query_request(path)])[0])


def queries(rpc, label, paths):
    """Many queries, ten to a request."""
    values = []
    for start in range(0, len(paths), MAX_BATCH):
        values += [query_value(r) for r in rpc.call(label, [query_request(p) for p in paths[start:start + MAX_BATCH]])]
    return values


def supply_rule(view):
    """DRT total is genesis plus emitted minus burned; all DGT issued, none minted."""
    drt, dgt = view['supply'], view['dgt_supply']
    return (int(drt['total']) == int(drt['genesis']) + int(drt['emitted']) - int(drt['burned'])
            and int(dgt['issued']) == DGT_TOTAL)


def status_reading(view):
    return {'height': int(view['height']), 'app_hash': view.get('app_hash'), 'supply': view['supply'],
            'dgt_supply': view['dgt_supply']}


# Offline signing with the release's CLI

def to_bytes(hex32):
    return list(bytes.fromhex(hex32))


def signing_inputs(profile, account, nonce):
    """The account view and signing context to sign at `nonce` against a
    captured profile view: the account's authority with the locally tracked
    nonce, at the profile's committed context. These are the signer's own
    expectations; the node checks the nonce and everything else again."""
    account = dict(account, context=profile['context'], spending_nonce=str(nonce))
    domain = account['domain']
    context = {'domain': {'network': domain['network'], 'chain_id': domain['chain_id'],
                          'genesis_digest': to_bytes(domain['genesis_digest']),
                          'account_id': to_bytes(domain['account_id'])},
               'current_key': account['current_key'],
               'authorization_generation': str(account['authorization_generation']),
               'spending_nonce': str(nonce), 'committed': profile['context'],
               'profile_digest': to_bytes(account['profile_digest']), 'protected': account['protected']}
    return account, context


class Signer:
    """`ordinary prepare`, `sign` and `transport`, offline, one scratch
    directory per transaction."""

    def __init__(self, cli, env, scratch):
        self.cli, self.env, self.scratch = cli, env, Path(scratch)
        self.scratch.mkdir(parents=True, exist_ok=True, mode=0o700)

    def run(self, *args):
        result = subprocess.run([str(self.cli), *map(str, args)], capture_output=True, text=True, env=self.env)
        if result.returncode != 0:
            raise Failed(f'dytallix {" ".join(map(str, args[:2]))} failed: {result.stderr.strip()[:500]}')
        return json.loads(result.stdout.strip().splitlines()[-1])

    def sign(self, profile_path, profile, account, nonce, actions, expiry, gas_limit, cap, key_file=None,
             wallet=None, memo='t3'):
        """(transaction ID, transport bytes)."""
        work = Path(tempfile.mkdtemp(dir=self.scratch))
        try:
            account, context = signing_inputs(profile, account, nonce)
            (work / 'account.json').write_text(json.dumps(account))
            (work / 'context.json').write_text(json.dumps(context))
            (work / 'actions.json').write_text(json.dumps(actions))
            inputs = ('--profile', profile_path, '--account', work / 'account.json', '--context', work / 'context.json')
            prepared = self.run('ordinary', 'prepare', *inputs, '--actions', work / 'actions.json', '--memo', memo,
                                '--expiry-height', expiry, '--gas-limit', gas_limit, '--maximum-fee-udrt', cap,
                                '--output', work / 'body.json')
            signer = ('--key-file', key_file) if key_file else ('--wallet', wallet)
            self.run('ordinary', 'sign', *inputs, '--body', work / 'body.json', *signer, '--output', work / 'signed.json')
            self.run('ordinary', 'transport', *inputs, '--signed', work / 'signed.json', '--output', work / 'tx.bin')
            return prepared['transaction_id'], (work / 'tx.bin').read_bytes()
        finally:
            shutil.rmtree(work, ignore_errors=True)


# Wallets, the mix and the actions

class Wallet:
    def __init__(self, label, role, address, account_id, key_file=None, keystore_name=None):
        self.label, self.role, self.address, self.account_id = label, role, address, account_id
        self.key_file, self.keystore_name = key_file, keystore_name
        self.view = None        # the account view the signer starts from
        self.nonce = None       # the locally tracked spending nonce
        self.drt = self.dgt = 0  # what the wallet may still spend, as tracked
        self.bonded, self.bond_height = 0, None
        self.busy = False
        self.needs_sync = False


def parse_mix(text):
    mix = {}
    for part in text.split(','):
        kind, _, weight = part.partition('=')
        kind = kind.strip()
        if kind not in KINDS or not weight.strip().isdigit():
            raise Failed(f'--mix takes KIND=WEIGHT from {", ".join(KINDS)}: {part!r}')
        mix[kind] = int(weight)
    if not any(mix.values()):
        raise Failed('--mix needs a positive weight')
    return mix


def eligible(wallet, kind, head, reserve, spares_left):
    """Whether the wallet can make a valid transaction of this kind, from what
    it is tracked to hold: the fee cap is reserved first."""
    drt = wallet.drt - reserve
    settled = wallet.bond_height is not None and head > wallet.bond_height + 2
    return {'send_drt': drt >= AMOUNTS['send_drt'],
            'send_dgt': drt >= 0 and wallet.dgt >= AMOUNTS['send_dgt'],
            'bond': drt >= 0 and wallet.dgt >= AMOUNTS['bond'],
            'unbond': drt >= 0 and settled and wallet.bonded >= AMOUNTS['unbond'],
            'claim': drt >= 0 and settled and wallet.bonded > 0,
            'create': drt >= AMOUNTS['create'] and spares_left > 0}[kind]


def choose(rng, wallet, mix, head, reserve, spares_left):
    kinds = [k for k, w in mix.items() if w > 0 and eligible(wallet, k, head, reserve, spares_left)]
    if not kinds:
        return None
    return rng.choices(kinds, weights=[mix[k] for k in kinds])[0]


def send_action(recipient_hex, denomination, amount):
    return {'type': 'Send', 'recipient': to_bytes(recipient_hex), 'denomination': denomination, 'amount': str(amount)}


def actions_for(kind, recipient_hex=None, validator=None):
    if kind in ('send_drt', 'create'):
        return [send_action(recipient_hex, 'udrt', AMOUNTS[kind])]
    if kind == 'send_dgt':
        return [send_action(recipient_hex, 'udgt', AMOUNTS[kind])]
    if kind == 'bond':
        return [{'type': 'RewardBond', 'validator_id': validator, 'amount_udgt': str(AMOUNTS['bond'])}]
    if kind == 'unbond':
        return [{'type': 'RewardBeginUnbond', 'validator_id': validator, 'amount_udgt': str(AMOUNTS['unbond'])}]
    if kind == 'claim':
        return [{'type': 'RewardClaim'}]
    raise Failed(f'unknown kind {kind}')


# The record of every transaction

def new_row(phase, kind, wallet, path='offline'):
    return {'phase': phase, 'kind': kind, 'wallet': wallet.label, 'path': path, 'nonce': None, 'id': None,
            'engine_hash': None, 'expiry_height': None, 'submit_height': None, 'submitted_at': None,
            'broadcast_ms': None, 'code': None, 'log': None, 'status': 'signed', 'block_height': None,
            'outcome': None, 'charge_udrt': None, 'created_accounts': 0, 'receipt_seen_height': None}


def apply_receipt(row, receipt, account_id):
    """A committed receipt for a pending row. Its identity must match."""
    if (receipt['transaction_id'] != row['id'] or receipt['actor'] != account_id
            or (row['nonce'] is not None and int(receipt['nonce_before']) != row['nonce'])):
        raise Failed(f'the receipt for {row["id"]} is for another transaction: {receipt}')
    row.update(status='committed', block_height=int(receipt['block_height']), outcome=receipt['outcome'],
               charge_udrt=str(receipt['charge']), receipt_seen_height=int(receipt['context']['height']))
    if receipt['outcome'] != 'success':
        row['created_accounts'] = 0
    return int(receipt['nonce_after'])


class Tracker:
    """Pending transactions and their receipts, polled ten to a request
    with the head height."""

    def __init__(self, rpc, wallets):
        self.rpc, self.wallets = rpc, wallets
        self.pending = {}
        self.head = 0
        self.lock = threading.Lock()

    def add(self, row):
        with self.lock:
            self.pending[row['id']] = row

    def due(self, head):
        """Pending rows whose receipt could exist and that were not read at
        this head yet."""
        with self.lock:
            return [r for r in self.pending.values()
                    if (r['submit_height'] or 0) < head and r.get('_polled_at', -1) < head][:MAX_BATCH - 1]

    def poll(self, rows=None):
        """One request: the head, then up to nine receipts. Returns the rows
        resolved: committed, or lost once their expiry has passed."""
        rows = self.due(self.head) if rows is None else rows
        results = self.rpc.call('receipts', [('abci_info', {})] +
                                [query_request(f'/ordinary/receipt/{r["id"]}') for r in rows])
        head = head_height(results[0])
        self.head = max(self.head, head)
        resolved = []
        for row, result in zip(rows, results[1:]):
            try:
                receipt = query_value(result)
            except Failed:
                continue
            row['_polled_at'] = head
            wallet = self.wallets[row['wallet']]
            if receipt is not None:
                wallet.nonce = apply_receipt(row, receipt, wallet.account_id)
                resolved.append(row)
            elif head >= row['expiry_height']:
                # A transaction commits only below its expiry height: absent at
                # that height, it never will.
                row['status'] = 'lost'
                resolved.append(row)
        with self.lock:
            for row in resolved:
                self.pending.pop(row['id'], None)
        return resolved

    def drain(self, timeout, sleep=time.sleep, clock=time.monotonic, interval=1.0):
        resolved, deadline = [], clock() + timeout
        while self.pending:
            if clock() > deadline:
                raise Failed(f'{len(self.pending)} transactions unresolved after {timeout} s')
            rows = self.due(self.head)
            try:
                resolved += self.poll(rows)
            except Transport:
                pass
            if not self.due(self.head):
                sleep(interval)
        return resolved


def broadcast(rpc, signed, label='broadcast'):
    """One request: the head height, then each signed transaction. Each row
    gets its submission height, admission code and response time; refused
    rows carry the node's reason. Transport marks the batch's outcome as
    unknown; the rows stay pending to their expiry."""
    requests = [('abci_info', {})] + [('broadcast_tx_sync', {'tx': base64.b64encode(tx).decode()}) for _, tx in signed]
    submitted_at = time.time()
    try:
        results, elapsed = rpc.timed(label, requests)
    except Transport as error:
        for row, _ in signed:
            row.update(status='uncertain', submitted_at=submitted_at, log=str(error)[:200])
        raise
    head = head_height(results[0])
    for (row, tx), result in zip(signed, results[1:]):
        row.update(submitted_at=submitted_at, submit_height=head, broadcast_ms=round(elapsed, 1),
                   engine_hash=sha256(tx))
        if isinstance(result, RpcError):
            row.update(status='refused', code=-1, log=str(result)[:200])
        elif int(result['code']) != 0:
            row.update(status='refused', code=int(result['code']), log=(result.get('log') or '')[:200])
        else:
            if (result.get('hash') or '').lower() != row['engine_hash']:
                raise Failed(f'the node reports hash {result.get("hash")} for {row["id"]}')
            row.update(status='admitted', code=0)
    return head


# One run

class Settings:
    def __init__(self, **values):
        self.rate = 1.0
        self.duration = 120.0
        self.mix = parse_mix(DEFAULT_MIX)
        self.connections = 2
        self.batch = MAX_BATCH
        self.signers = 4
        self.expiry_blocks = 20
        self.fund_drt = 200 * UNIT
        self.fund_dgt = 500 * UNIT
        self.governance = False
        self.governance_vote = False
        # Reward sweeps: about hourly, keeping this much liquid DRT.
        self.sweep_interval = 3600.0
        self.sweep_keep = 100 * UNIT
        self.governance_bond = 100 * UNIT
        self.seed = 1
        self.min_within_2 = 0.99
        self.max_refused = None
        self.block_time = 5.0
        # How often the head and the views are read when nothing is due.
        self.poll_interval = 1.0
        self.__dict__.update(values)


class Load:
    """The fleet's wallets, the views they sign against and the record."""

    def __init__(self, rpc, signer, cli_env, cli, fleet, out, settings, funder, validator, config, show=say,
                 clock=time.monotonic, sleep=time.sleep, sweepers=()):
        self.rpc, self.signer, self.env, self.cli = rpc, signer, cli_env, cli
        self.out, self.s, self.validator, self.show = Path(out), settings, validator, show
        self.clock, self.sleep = clock, sleep
        self.fleet_dir = Path(fleet['_dir'])
        self.wallets = {}
        for record in fleet['wallets']:
            self.wallets[record['label']] = Wallet(record['label'], record['role'], record['address'],
                                                   record['account_id'], self.fleet_dir / record['key_file'])
        self.funder = Wallet(funder['label'], 'funder', funder['address'], funder['account_id'],
                             keystore_name=funder['label'])
        self.wallets[self.funder.label] = self.funder
        # Keystore accounts whose rewards go back to the funder.
        self.sweepers = [Wallet(a['label'], 'sweeper', a['address'], a['account_id'], keystore_name=a['label'])
                         for a in sweepers]
        for wallet in self.sweepers:
            self.wallets[wallet.label] = wallet
        self.sweeps = []
        self.rows, self.rows_lock = [], threading.Lock()
        self.tracker = Tracker(rpc, self.wallets)
        self.rng = random.Random(settings.seed)
        fee = config['ordinary']['fee_profile']
        self.creation_fee = int(fee['account_creation_fee_udrt'])
        self.gas_price = int(fee['gas_price'])
        self.cap = GAS_LIMIT * self.gas_price
        self.max_active = int(config['lifecycle']['max_active'])
        self.minimum_deposit = int(config['governance']['deposit']['minimum_deposit_udgt'])
        self.views = None
        self.governance = {}
        self.last_poll = None

    def role(self, *roles):
        return [w for w in self.wallets.values() if w.role in roles]

    def record(self, row):
        with self.rows_lock:
            self.rows.append(row)
        return row

    # Views

    def refresh_profile(self):
        profile = query(self.rpc, 'profile', '/ordinary/profile')
        if not profile.get('enabled'):
            raise Failed('ordinary transactions are disabled on this chain')
        if self.profile and profile['config']['fee_profile'] != self.profile['config']['fee_profile']:
            for wallet in self.wallets.values():
                wallet.needs_sync = True
        path = self.out / 'views' / f'profile-{profile["context"]["height"]}.json'
        path.parent.mkdir(exist_ok=True)
        if not path.exists():
            path.write_text(json.dumps(profile))
        # One assignment: a signer reads the pair once.
        self.views = (path, profile)
        self.tracker.head = max(self.tracker.head, int(profile['context']['height']))

    @property
    def profile(self):
        return self.views[1] if self.views else None

    def profile_stale(self):
        return int(self.profile['context']['height']) + self.s.expiry_blocks // 2 <= self.tracker.head

    def sync(self, wallets):
        """Each wallet's account view and committed nonce."""
        views = queries(self.rpc, 'accounts', [f'/ordinary/account/{w.account_id}' for w in wallets])
        for wallet, view in zip(wallets, views):
            if view is None:
                raise Failed(f'{wallet.label} has no account')
            if view['account_id'] != wallet.account_id:
                raise Failed(f'the node returned another account for {wallet.label}')
            wallet.view, wallet.nonce, wallet.needs_sync = view, int(view['spending_nonce']), False

    def sign(self, row, wallet, actions, gas_limit=GAS_LIMIT):
        """Signs the row's transaction at the wallet's tracked nonce against
        the newest profile view; the expiry follows that view."""
        path, profile = self.views
        expiry = int(profile['context']['height']) + 1 + self.s.expiry_blocks
        row.update(nonce=wallet.nonce, expiry_height=expiry)
        row['id'], tx = self.signer.sign(path, profile, wallet.view, wallet.nonce, actions, expiry, gas_limit,
                                         gas_limit * self.gas_price, key_file=wallet.key_file,
                                         wallet=wallet.keystore_name)
        return row, tx

    def wait(self, rows, timeout):
        for row in rows:
            if row['status'] in ('admitted', 'uncertain'):
                self.tracker.add(row)
        self.tracker.drain(timeout, sleep=self.sleep, clock=self.clock, interval=self.s.poll_interval)
        failed = [r for r in rows if r['status'] != 'committed' or r['outcome'] != 'success']
        if failed:
            raise Failed(f'{len(failed)} setup transactions failed: {failed[0]}')

    # Setup

    def fund(self):
        """Funds the load wallets (and, with --governance, the governance
        wallets). Without distributors the funder pays them all, sixteen
        transfers a transaction, one transaction at a time. With them it
        fans out (P01, 12 October 2026): the funder pays each distributor
        what its share needs, each distributor makes its priming spend, then
        every distributor pays its share at once, one transaction in flight
        each. The transfers create the accounts."""
        self.refresh_profile()
        self.sync([self.funder])
        plan = [(w, self.s.fund_drt, self.s.fund_dgt) for w in self.role('load')]
        if self.s.governance:
            plan += [(w, self.s.fund_drt, self.s.governance_bond) for w in self.role('proposer', 'voter')]
            plan += [(w, self.s.fund_drt, self.s.governance_bond + self.minimum_deposit) for w in self.role('depositor')]
        distributors = self.role('distributor')
        if not distributors:
            self.show(f'== funding {len(plan)} wallets from {self.funder.label}')
            self.pay({self.funder: plan})
            return
        shares = {d: plan[n::len(distributors)] for n, d in enumerate(distributors)}
        self.show(f'== funding {len(plan)} wallets through {len(distributors)} distributors '
                  f'(up to {max(len(v) for v in shares.values())} each)')
        self.pay({self.funder: [(d, *self.distributor_needs(share)) for d, share in shares.items()]})
        self.prime([(d, ('send', '--to', self.funder.address, '--amount', '1', '--token', 'drt'))
                    for d in distributors])
        self.pay({d: share for d, share in shares.items() if share})

    def distributor_needs(self, share):
        """What a distributor needs to pay its share: the transfers, each
        created account's fee, a full fee cap for each of its transactions
        and its priming spend (1 DRT and its cap)."""
        transactions = -(-2 * len(share) // FUNDING_ACTIONS)
        drt = (sum(d for _, d, _ in share) + len(share) * self.creation_fee
               + transactions * FUNDING_GAS_LIMIT * self.gas_price + self.cap + UNIT)
        return drt, sum(g for _, _, g in share)

    def pay(self, plan, timeout=None):
        """Each payer pays its recipients [(wallet, udrt, udgt)], sixteen
        transfers a transaction; the payers run in parallel, one transaction
        in flight each, broadcast together."""
        timeout = timeout or self.s.block_time * (self.s.expiry_blocks + 6) * (
            1 + max(-(-2 * len(r) // FUNDING_ACTIONS) for r in plan.values()))
        per_tx = FUNDING_ACTIONS // 2
        queues = {payer: deque(recipients[n:n + per_tx] for n in range(0, len(recipients), per_tx))
                  for payer, recipients in plan.items()}
        pending, deadline = {}, self.clock() + timeout
        with ThreadPoolExecutor(max_workers=self.s.signers) as pool:
            while any(queues.values()) or pending:
                if self.clock() > deadline:
                    raise Failed(f'funding unfinished after {timeout} s: {len(pending)} in flight')
                ready = [p for p in queues if queues[p] and p not in pending]
                if ready:
                    if self.profile_stale():
                        self.refresh_profile()
                    jobs = []
                    for payer in ready:
                        chunk = queues[payer].popleft()
                        actions = [a for w, drt, dgt in chunk for a in
                                   (send_action(w.account_id, 'udrt', drt), send_action(w.account_id, 'udgt', dgt))]
                        row = self.record(new_row('setup', 'fund', payer))
                        row['created_accounts'], row['recipients'] = len(chunk), [w.label for w, _, _ in chunk]
                        row['_chunk'] = chunk
                        jobs.append(pool.submit(self.sign, row, payer, actions, FUNDING_GAS_LIMIT))
                    signed = [job.result() for job in jobs]
                    for n in range(0, len(signed), MAX_BATCH - 1):
                        batch = signed[n:n + MAX_BATCH - 1]
                        broadcast(self.rpc, batch, 'setup')
                        for row, _ in batch:
                            if row['status'] != 'admitted':
                                raise Failed(f'a funding transaction was refused: {row["log"]}')
                            self.tracker.add(row)
                            pending[self.wallets[row['wallet']]] = row
                resolved = self.tracker.poll(self.tracker.due(self.tracker.head))
                for row in resolved:
                    if row['status'] != 'committed' or row['outcome'] != 'success':
                        raise Failed(f'a funding transaction failed: {row}')
                    pending.pop(self.wallets[row['wallet']])
                    for wallet, drt, dgt in row.pop('_chunk'):
                        wallet.drt, wallet.dgt = wallet.drt + drt, wallet.dgt + dgt
                if not resolved:
                    self.sleep(self.s.poll_interval)

    def online(self, wallet, args, phase='setup', kind=None):
        """One write through the CLI's online path, holding a channel slot
        for the whole call: a wallet's first spend, a governance transaction
        or a reward sweep. A keystore wallet signs with --wallet. Without the
        signed body in its report, the row's expiry is bounded from the head
        read after the call."""
        row = self.record(new_row(phase, kind or args[0], wallet, path='online'))
        row['submit_height'] = self.tracker.head
        signer = ('--wallet', wallet.keystore_name) if wallet.keystore_name else ('--key-file', str(wallet.key_file))
        started = self.clock()
        with self.rpc.slots:
            result = subprocess.run([str(self.cli), *map(str, args), *signer,
                                     '--gas-limit', str(GAS_LIMIT), '--maximum-fee-udrt', str(self.cap),
                                     '--expiry-blocks', str(self.s.expiry_blocks), '--wait-seconds', '0'],
                                    capture_output=True, text=True, env=self.env)
        row.update(broadcast_ms=round((self.clock() - started) * 1000, 1), submitted_at=time.time())
        if result.returncode != 0:
            row.update(status='refused', code=-1, log=(result.stderr.strip() or result.stdout.strip())[:200])
            return row
        report = json.loads(result.stdout.strip().splitlines()[-1])
        check = report['check_tx']
        row.update(id=report['transaction_id'], code=int(check['code']), log=(check.get('log') or '')[:200],
                   engine_hash=(check.get('engine_hash') or '').lower() or None,
                   status='admitted' if int(check['code']) == 0 else 'refused')
        body = report.get('body')
        if body:
            row.update(nonce=int(body['spending_nonce']), expiry_height=int(body['expiry_height']))
        else:
            # The CLI's context height is at most the head after the call.
            head = head_height(self.rpc.one('head', 'abci_info', {}))
            row['expiry_height'] = head + 1 + self.s.expiry_blocks
        return row

    def prime(self, jobs):
        """Each wallet's first spend through the CLI's online path, before
        the measured window, then its account view."""
        self.show(f'== {len(jobs)} priming spends through the online path')
        self.tracker.head = head_height(self.rpc.one('head', 'abci_info', {}))
        with ThreadPoolExecutor(max_workers=self.rpc.connections) as pool:
            rows = list(pool.map(lambda job: self.online(job[0], job[1], kind='first_spend'), jobs))
        for row in rows:
            row['nonce'] = 0
            if row['status'] != 'admitted':
                raise Failed(f'{row["wallet"]}\'s first spend was refused: {row["log"]}')
        self.wait(rows, self.s.block_time * (self.s.expiry_blocks + 6))
        for (wallet, args), row in zip(jobs, rows):
            wallet.drt -= int(row['charge_udrt'])
            if args[0] == 'send':
                wallet.drt -= UNIT
                recipient = next((w for w in self.wallets.values() if w.address == args[2]), None)
                if recipient:
                    recipient.drt += UNIT
            else:
                wallet.dgt -= self.s.governance_bond
                wallet.bonded, wallet.bond_height = self.s.governance_bond, row['block_height']
        self.sync([w for w, _ in jobs])
        return rows

    def first_spends(self):
        """The priming spends: a load wallet sends 1 DRT to the next one, a
        governance wallet bonds."""
        loads = self.role('load')
        if len(loads) < 2:
            raise Failed('the run needs at least two load wallets')
        jobs = [(w, ('send', '--to', loads[(n + 1) % len(loads)].address, '--amount', '1', '--token', 'drt'))
                for n, w in enumerate(loads)]
        if self.s.governance:
            bond = f'{self.s.governance_bond / UNIT:.6f}'
            jobs += [(w, ('stake', 'bond', '--validator', self.validator, '--amount', bond))
                     for w in self.role('proposer', 'depositor', 'voter')]
        self.prime(jobs)
        if self.s.governance:
            self.wait_bonded(self.role('proposer', 'depositor', 'voter'))

    # Reward sweeps (P01, 12 October 2026): the genesis allocation stays the
    # production one, so the operators' and stakers' rewards go back to the
    # funder about hourly.

    def sweep(self, keep):
        """Each sweeper claims its rewards, then sends its liquid DRT above
        `keep` to the funder, both through the CLI's online path. Returns
        what each returned."""
        returned = {}
        for wallet in self.sweepers:
            if int(self.summary(wallet)['claimable_rewards_udrt']) > 0:
                self.follow(self.online(wallet, ('stake', 'claim'), phase='sweep', kind='sweep_claim'))
            surplus = int(self.summary(wallet)['liquid_udrt']) - keep
            if surplus > 0:
                row = self.follow(self.online(wallet, ('send', '--to', self.funder.address, '--amount',
                                                       f'{surplus / UNIT:.6f}', '--token', 'drt'),
                                              phase='sweep', kind='sweep_return'))
                row['amount_udrt'] = str(surplus)
                returned[wallet.label] = str(surplus)
        return returned

    def follow(self, row, timeout=None):
        """An online row to its receipt."""
        if row['status'] != 'admitted':
            raise Failed(f'{row["wallet"]}\'s {row["kind"]} was refused: {row["log"]}')
        deadline = self.clock() + (timeout or self.s.block_time * (self.s.expiry_blocks + 6))
        wallet = self.wallets[row['wallet']]
        while True:
            receipt = query(self.rpc, 'sweep', f'/ordinary/receipt/{row["id"]}')
            if receipt is not None:
                apply_receipt(row, receipt, wallet.account_id)
                if row['outcome'] != 'success':
                    raise Failed(f'{row["wallet"]}\'s {row["kind"]} failed: {receipt.get("rule_code")}')
                return row
            head = head_height(self.rpc.one('head', 'abci_info', {}))
            if head >= row['expiry_height'] or self.clock() > deadline:
                row['status'] = 'lost'
                raise Failed(f'{row["wallet"]}\'s {row["kind"]} was lost')
            self.sleep(self.s.poll_interval)

    def wait_bonded(self, wallets, timeout=None):
        """A bond joins the set two blocks after it commits; a proposer and a
        voter need it in effect."""
        deadline = self.clock() + (timeout or self.s.block_time * 20)
        for wallet in wallets:
            while int(self.summary(wallet)['bonded_udgt']) < self.s.governance_bond:
                if self.clock() > deadline:
                    raise Failed(f'{wallet.label}\'s bond is not in effect')
                self.sleep(self.s.poll_interval)

    # The measured window

    def window(self):
        s = self.s
        loads = self.role('load')
        spares = deque(self.role('spare'))
        idle = deque(loads)
        reserve = self.cap
        signing, sending = {}, deque()
        rpc_jobs = {}
        signers = ThreadPoolExecutor(max_workers=s.signers)
        rpcs = ThreadPoolExecutor(max_workers=s.connections)
        self.refresh_profile()
        started = self.clock()
        tokens, last, stopped = 0.0, started, False
        skipped = 0
        self.show(f'== the measured window: {s.rate} transactions a second for {s.duration} s over {len(loads)} wallets')
        try:
            while True:
                now = self.clock()
                if not stopped and now - started >= s.duration:
                    stopped = True
                    self.show(f'== {sum(1 for r in self.rows if r["phase"] == "run")} transactions launched; draining')
                if not stopped:
                    tokens = min(tokens + s.rate * (now - last), max(s.batch * 2, 1))
                last = now
                # Launch: the next idle wallet, a kind it can make, signed offline.
                while not stopped and tokens >= 1:
                    tokens -= 1
                    wallet = idle.popleft() if idle else None
                    if wallet is None:
                        skipped += 1
                        continue
                    if wallet.needs_sync:
                        idle.append(wallet)
                        skipped += 1
                        continue
                    kind = choose(self.rng, wallet, s.mix, self.tracker.head, reserve, len(spares))
                    if kind is None:
                        skipped += 1
                        continue  # exhausted: the wallet leaves the rotation
                    recipient = None
                    row = new_row('run', kind, wallet)
                    if kind == 'create':
                        recipient = spares.popleft()
                        row['created_accounts'], row['recipient'] = 1, recipient.label
                    elif kind in ('send_drt', 'send_dgt'):
                        others = [w for w in loads if w is not wallet]
                        recipient = self.rng.choice(others)
                        row['recipient'] = recipient.label
                    wallet.busy = True
                    self.record(row)
                    actions = actions_for(kind, recipient.account_id if recipient else None, self.validator)
                    signing[signers.submit(self.sign, row, wallet, actions)] = (row, wallet)
                # Signed transactions wait for a channel slot.
                for future in [f for f in signing if f.done()]:
                    row, wallet = signing.pop(future)
                    try:
                        sending.append(future.result())
                    except Failed as error:
                        row.update(status='unsigned', log=str(error)[:200])
                        wallet.busy, wallet.needs_sync = False, True
                        idle.append(wallet)
                # Finished requests.
                for future in [f for f in rpc_jobs if f.done()]:
                    what, payload = rpc_jobs.pop(future)
                    try:
                        result = future.result()
                    except Transport as error:
                        result = error
                    except Failed as error:
                        raise Failed(f'{what}: {error}')
                    if what == 'broadcast':
                        for row, _ in payload:
                            if row['status'] in ('admitted', 'uncertain'):
                                if row['status'] == 'uncertain':
                                    row['submit_height'] = self.tracker.head
                                self.tracker.add(row)
                            else:
                                wallet = self.wallets[row['wallet']]
                                wallet.busy = False
                                wallet.needs_sync = True
                                idle.append(wallet)
                    elif what == 'poll' and not isinstance(result, Exception):
                        for row in result:
                            self.settle(row, idle)
                # Start requests while slots are free: broadcasts first, then
                # account syncs, the profile, and receipts.
                finished = stopped and not signing and not sending and not self.tracker.pending
                while not finished and len(rpc_jobs) < s.connections:
                    if sending:
                        batch = [sending.popleft() for _ in range(min(len(sending), s.batch - 1))]
                        rpc_jobs[rpcs.submit(broadcast, self.rpc, batch)] = ('broadcast', batch)
                        continue
                    stale = [w for w in idle if w.needs_sync][:MAX_BATCH]
                    if stale and not any(what == 'sync' for what, _ in rpc_jobs.values()):
                        rpc_jobs[rpcs.submit(self.sync, stale)] = ('sync', stale)
                        continue
                    if self.profile_stale() and not any(what == 'profile' for what, _ in rpc_jobs.values()):
                        rpc_jobs[rpcs.submit(self.refresh_profile)] = ('profile', None)
                        continue
                    due = self.tracker.due(self.tracker.head)
                    polling = any(what == 'poll' for what, _ in rpc_jobs.values())
                    if not polling and (due or self.last_poll is None or now - self.last_poll >= s.poll_interval):
                        self.last_poll = now
                        rpc_jobs[rpcs.submit(self.tracker.poll, due)] = ('poll', due)
                        continue
                    break
                if finished and not rpc_jobs:
                    break
                if stopped and now - started > s.duration + s.block_time * (s.expiry_blocks + 6) + 60:
                    raise Failed(f'{len(self.tracker.pending)} transactions unresolved after the window')
                self.sleep(0.05)
        finally:
            signers.shutdown(wait=True)
            rpcs.shutdown(wait=True)
        return {'started': started, 'ended': self.clock(), 'skipped_launches': skipped}

    def settle(self, row, idle):
        """A resolved run transaction frees its wallet, with what it holds as
        tracked."""
        wallet = self.wallets[row['wallet']]
        wallet.busy = False
        if row['status'] == 'committed':
            wallet.drt -= int(row['charge_udrt'])
            if row['outcome'] == 'success':
                kind = row['kind']
                recipient = self.wallets.get(row.get('recipient'))
                if kind in ('send_drt', 'create'):
                    wallet.drt -= AMOUNTS[kind]
                    if recipient:
                        recipient.drt += AMOUNTS[kind]
                elif kind == 'send_dgt':
                    wallet.dgt -= AMOUNTS[kind]
                    recipient.dgt += AMOUNTS[kind]
                elif kind == 'bond':
                    wallet.dgt -= AMOUNTS['bond']
                    wallet.bonded += AMOUNTS['bond']
                    wallet.bond_height = row['block_height']
                elif kind == 'unbond':
                    wallet.bonded -= AMOUNTS['unbond']
        else:
            wallet.needs_sync = True
        idle.append(wallet)

    # Governance (ordinary v3: no receipts yet)

    def summary(self, wallet):
        return query(self.rpc, 'governance', f'/account/{wallet.address}')

    def governance_step(self, wallet, what, args, timeout):
        """One v3 transaction on a governance wallet; its charge is the fall
        in the wallet's liquid DRT from its view at the spent nonce to its
        view after, which nothing else changes."""
        before = self.summary(wallet)
        row = self.online(wallet, args, phase='governance', kind=what)
        row['liquid_before_udrt'] = before['liquid_udrt']
        if row['status'] != 'admitted':
            return row
        if row['nonce'] != int(before['nonce']):
            raise Failed(f'{wallet.label} signed nonce {row["nonce"]}, its view shows {before["nonce"]}')
        deadline = self.clock() + timeout
        while True:
            after = self.summary(wallet)
            if int(after['nonce']) > row['nonce']:
                break
            if int(after['context']['height']) >= row['expiry_height'] or self.clock() > deadline:
                row['status'] = 'lost'
                return row
            self.sleep(self.s.poll_interval)
        # No v3 receipt: the outcome is only that the nonce was spent.
        row.update(governance_charge(before, after, row['nonce']), status='committed', outcome='nonce_spent')
        return row

    def run_governance(self, timeout=None):
        """Propose a no-op change (the active set size the chain already
        has) and deposit its minimum. Its vote opens only when the deposit
        period closes (720 blocks under the rehearsal timings, 120,960 in
        production): with --governance-vote the voters wait for it and vote
        yes; otherwise the proposal is left collecting, and recorded."""
        timeout = timeout or self.s.block_time * 40
        proposer, depositor = self.role('proposer')[0], self.role('depositor')[0]
        voters = self.role('voter')
        governance = query(self.rpc, 'governance', '/ordinary/profile_v3')
        if not governance.get('enabled'):
            raise Failed('governance is disabled on this chain')
        proposal = int(governance['next_proposal_id'])
        steps = []
        self.governance = {'proposal_id': proposal, 'steps': steps}

        def record(row):
            steps.append({k: row[k] for k in ('kind', 'wallet', 'nonce', 'status', 'charge_udrt')})
            if row['status'] != 'committed':
                raise Failed(f'{row["kind"]} is {row["status"]}: {row["log"]}')

        record(self.governance_step(proposer, 'governance_propose',
                                    ('governance', 'propose', '--max-active', self.max_active), timeout))
        view = query(self.rpc, 'governance', f'/governance/proposal/{proposal}')
        if view is None or view['proposer'] != proposer.account_id:
            raise Failed(f'proposal {proposal} is not the proposer\'s: {view}')
        record(self.governance_step(depositor, 'governance_deposit',
                                    ('governance', 'deposit', '--proposal-id', proposal, '--amount',
                                     f'{self.minimum_deposit / UNIT:.6f}'), timeout))
        view = query(self.rpc, 'governance', f'/governance/proposal/{proposal}')
        self.governance.update(phase=view['phase']['phase'], deposited_udgt=str(view['deposited_udgt']),
                               due_height=int(view['due_height']), votes=[])
        if not self.s.governance_vote:
            return
        # The vote opens at the deposit close: wait for it, a block at a time.
        wait = self.s.block_time * (max(int(view['due_height']) - int(view['context']['height']), 0) + 20)
        deadline = self.clock() + wait
        while view['phase']['phase'] != 'voting':
            if self.clock() > deadline or view['phase']['phase'] != 'collecting':
                raise Failed(f'proposal {proposal} did not reach its vote: {view["phase"]}')
            self.sleep(max(self.s.poll_interval, self.s.block_time))
            view = query(self.rpc, 'governance', f'/governance/proposal/{proposal}')
        for voter in voters:
            record(self.governance_step(voter, 'governance_vote',
                                        ('governance', 'vote', '--proposal-id', proposal, '--choice', 'yes'), timeout))
        votes = [query(self.rpc, 'governance', f'/governance/vote/{proposal}/{v.account_id}') for v in voters]
        self.governance.update(phase='voting', votes=[v and v.get('choice') for v in votes])

    # Afterwards

    def status(self, label):
        return query(self.rpc, label, '/status')

    def block_times(self, low, high):
        """Each block's header time from low to high."""
        times = {}
        heights = list(range(max(low, 1), high + 1))
        for start in range(0, len(heights), MAX_BATCH):
            chunk = heights[start:start + MAX_BATCH]
            for height, result in zip(chunk, self.rpc.call('headers', [('header', {'height': str(h)}) for h in chunk])):
                if isinstance(result, RpcError):
                    raise result
                times[height] = result['header']['time']
        return times


def governance_charge(before, after, nonce):
    """The charge of the transaction at `nonce`: the fall in liquid DRT from
    the view at that nonce to the first view after it."""
    if int(before['nonce']) != nonce or int(after['nonce']) != nonce + 1:
        raise Failed(f'the views do not bracket nonce {nonce} alone: {before["nonce"]} to {after["nonce"]}')
    charge = int(before['liquid_udrt']) - int(after['liquid_udrt'])
    if charge <= 0:
        raise Failed(f'no charge seen for nonce {nonce}: liquid DRT {before["liquid_udrt"]} to {after["liquid_udrt"]}')
    return {'charge_udrt': str(charge), 'liquid_after_udrt': after['liquid_udrt'],
            'receipt_seen_height': int(after['context']['height'])}


# The evidence

def header_seconds(text):
    """RFC 3339 with up to nanoseconds, in seconds since the epoch."""
    found = re.fullmatch(r'(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(?:\.(\d{1,9}))?Z', text)
    if not found:
        raise Failed(f'not a header time: {text}')
    whole = datetime.strptime(found.group(1), '%Y-%m-%dT%H:%M:%S').replace(tzinfo=timezone.utc).timestamp()
    return whole + int((found.group(2) or '0').ljust(9, '0')) / 1e9


def ledger(rows, from_height, creation_fee):
    """The reconciliation's ledger: every committed transaction from the
    earlier reading, its charge and the accounts it created."""
    entries = [{'what': f'{r["kind"]} {r["id"]}', 'charge_udrt': str(r['charge_udrt']),
                'created_accounts': r['created_accounts']}
               for r in rows if r['status'] == 'committed']
    return {'schema': LEDGER_SCHEMA, 'from_height': int(from_height), 'account_creation_fee_udrt': str(creation_fee),
            'entries': entries}


def summarize(rows, block_times):
    """Counts, inclusion delays, throughput, block intervals and response
    times, from the rows and header times alone."""
    run = [r for r in rows if r['phase'] == 'run']
    statuses = Counter(r['status'] for r in rows)
    delays = [r['block_height'] - r['submit_height'] for r in run
              if r['status'] == 'committed' and r['submit_height'] is not None]
    per_block = Counter(r['block_height'] for r in run if r['status'] == 'committed')
    if delays:
        low = min(r['submit_height'] for r in run if r['submit_height'] is not None) + 1
        high = max(per_block)
        blocks = [per_block.get(h, 0) for h in range(low, high + 1)]
    else:
        low = high = None
        blocks = []
    times = sorted((int(h), header_seconds(t)) for h, t in block_times.items())
    intervals = [b[1] - a[1] for a, b in zip(times, times[1:]) if b[0] == a[0] + 1]
    refusals = Counter((r['log'] or '')[:120] for r in rows if r['status'] == 'refused')
    return {
        'transactions': len(rows),
        'by_status': dict(sorted(statuses.items())),
        'by_phase': dict(sorted(Counter(r['phase'] for r in rows).items())),
        'run_by_kind': dict(sorted(Counter(r['kind'] for r in run).items())),
        'outcomes': dict(sorted(Counter(r['outcome'] for r in rows if r['status'] == 'committed').items())),
        'refusals': dict(sorted(refusals.items())),
        # Every transaction is committed or refused at admission; anything
        # else (lost, unresolved, never signed) is counted here.
        'lost': [f'{r["wallet"]}:{r["nonce"]}:{r["status"]}' for r in rows if r['status'] not in ('committed', 'refused')],
        'inclusion': {'samples': len(delays), 'within_1': sum(d <= 1 for d in delays),
                      'within_2': sum(d <= 2 for d in delays),
                      'share_within_2': round(sum(d <= 2 for d in delays) / len(delays), 6) if delays else None,
                      'blocks': distribution(delays, 0), 'histogram': {str(k): v for k, v in sorted(Counter(delays).items())}},
        'throughput': {'from_height': low, 'to_height': high, 'blocks': len(blocks), 'committed': sum(blocks),
                       'per_block_mean': round(sum(blocks) / len(blocks), 3) if blocks else None,
                       'per_block_max': max(blocks) if blocks else None},
        'block_interval_s': distribution(intervals, 3),
        'response_ms': {
            'broadcast': distribution([r['broadcast_ms'] for r in rows if r['path'] == 'offline' and r['broadcast_ms'] is not None]),
            'online': distribution([r['broadcast_ms'] for r in rows if r['path'] == 'online' and r['broadcast_ms'] is not None])},
    }


def evaluate(summary, ledger_record, before, after, governance, settings):
    """The checks, each PASS or FAIL with its detail."""
    checks = []

    def check(name, ok, detail):
        checks.append({'check': name, 'status': PASS if ok else FAIL, 'detail': detail})

    lost = summary['lost']
    by = summary['by_status']
    check('nothing_lost', not lost, f'{by.get("committed", 0)} committed, {by.get("refused", 0)} refused at '
          f'admission, {len(lost)} lost or unresolved')
    inclusion = summary['inclusion']
    share = inclusion['share_within_2']
    check('inclusion_within_2_blocks', share is not None and share >= settings['min_within_2'],
          f'{inclusion["within_2"]} of {inclusion["samples"]} committed within 2 blocks of submission '
          f'(share {share}, at least {settings["min_within_2"]})')
    interval = summary['block_interval_s']
    check('block_interval', interval['n'] > 0 and interval['p50'] <= BLOCK_INTERVAL_P50_S
          and interval['p99'] <= BLOCK_INTERVAL_P99_S,
          f'median {interval.get("p50")} s, 99th percentile {interval.get("p99")} s over {interval["n"]} intervals '
          f'(at most {BLOCK_INTERVAL_P50_S} and {BLOCK_INTERVAL_P99_S})')
    check('supply_rule', supply_rule(before) and supply_rule(after),
          f'DRT total = genesis + emitted - burned and every DGT issued, at heights {before["height"]} and {after["height"]}')
    burned = int(after['supply']['burned']) - int(before['supply']['burned'])
    charged = sum(int(e['charge_udrt']) for e in ledger_record['entries'])
    created = sum(int(e['created_accounts']) for e in ledger_record['entries'])
    expected = charged + created * int(ledger_record['account_creation_fee_udrt'])
    check('fees_and_burns', burned == expected,
          f'{burned} udrt burned from {before["height"]} to {after["height"]}; {len(ledger_record["entries"])} '
          f'transactions charged {charged} and created {created} accounts, {expected} expected')
    if settings.get('max_refused') is not None:
        refused = by.get('refused', 0)
        check('refusals', refused <= settings['max_refused'],
              f'{refused} refused at admission (at most {settings["max_refused"]}): {summary["refusals"]}')
    if settings.get('governance'):
        governance = governance or {}
        steps = governance.get('steps', [])
        kinds = {s['kind'] for s in steps if s['status'] == 'committed' and int(s['charge_udrt'] or 0) > 0}
        wanted = {'governance_propose', 'governance_deposit'}
        votes = governance.get('votes') or []
        voted = not settings.get('governance_vote') or (bool(votes) and all(v == 'yes' for v in votes))
        if settings.get('governance_vote'):
            wanted.add('governance_vote')
        check('governance', wanted <= kinds and voted and 'error' not in governance,
              f'proposal {governance.get("proposal_id")} ({governance.get("phase")}, due at '
              f'{governance.get("due_height")}): {[(s["kind"], s["status"], s["charge_udrt"]) for s in steps]}, '
              f'votes {votes}{", " + governance["error"] if "error" in governance else ""}')
    return checks


def evidence(chain_id, settings, fleet_record, rows_bytes, ledger_record, before, after, block_times, governance,
             window, started, finished):
    rows = [json.loads(line) for line in rows_bytes.splitlines() if line.strip()]
    summary = summarize(rows, block_times)
    checks = evaluate(summary, ledger_record, before, after, governance, settings)
    record = {'schema': SCHEMA, 'chain_id': chain_id, 'started': started, 'finished': finished,
              'settings': settings, 'fleet': fleet_record, 'window': window,
              'before': before, 'after': after, 'governance': governance, 'block_times': block_times,
              'transactions_sha256': sha256(rows_bytes), 'ledger_sha256': sha256(canonical(ledger_record)),
              'ledger': ledger_record, 'summary': summary, 'checks': checks,
              'status': PASS if all(c['status'] == PASS for c in checks) else FAIL}
    record['sha256'] = sha256(canonical(record))
    return record


def verify(record, rows_bytes):
    """Re-evaluates a run from its evidence and transactions file: the
    digests, the summary, the ledger and every check."""
    claimed = record.get('sha256')
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if sha256(canonical(body)) != claimed:
        raise Failed('the evidence digest does not match its contents')
    if sha256(rows_bytes) != record['transactions_sha256']:
        raise Failed('the transactions file is not the one the evidence names')
    if sha256(canonical(record['ledger'])) != record['ledger_sha256']:
        raise Failed('the ledger is not the one the evidence names')
    rows = [json.loads(line) for line in rows_bytes.splitlines() if line.strip()]
    if ledger(rows, record['ledger']['from_height'], record['ledger']['account_creation_fee_udrt']) != record['ledger']:
        raise Failed('the ledger does not follow from the transactions')
    if summarize(rows, record['block_times']) != record['summary']:
        raise Failed('the summary does not follow from the transactions')
    checks = evaluate(record['summary'], record['ledger'], record['before'], record['after'], record['governance'],
                      record['settings'])
    if checks != record['checks']:
        raise Failed('the checks do not follow from the evidence')
    return PASS if all(c['status'] == PASS for c in checks) else FAIL


def created_addresses(rows, wallets):
    """The addresses of the accounts committed transactions created."""
    labels = []
    for row in rows:
        if row['status'] == 'committed' and row['outcome'] == 'success':
            if row['kind'] == 'fund':
                labels += row['recipients']
            elif row['kind'] == 'create':
                labels.append(row['recipient'])
    return [wallets[label].address for label in labels]


def rows_jsonl(rows):
    return b''.join(canonical({k: v for k, v in r.items() if not k.startswith('_')}) + b'\n' for r in rows)


# The whole run

def keystore_account(entries, label, chain_id):
    """A keystore wallet's label, address and account ID on this chain."""
    if label not in entries:
        raise Failed(f'{label} is not in the keystore')
    account_id, address = config_checks.origin_account(TESTNET, chain_id, bytes(entries[label]['public_key']))
    return {'label': label, 'address': address, 'account_id': account_id.hex()}


def run_load(cli, home, fleet_dir, out, application_config, funder_label, validator, settings, show=say,
             port=0, passphrase=None, sweep_from=()):
    """Funding, first spends, the measured window (with governance beside
    it), then the evidence. Returns the evidence record. The funder's
    keystore passphrase file is `passphrase` or the environment's
    DYTALLIX_KEYSTORE_PASSPHRASE_FILE."""
    out, fleet_dir = Path(out), Path(fleet_dir)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(home))
    if passphrase:
        env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    config = json.loads(Path(application_config).read_bytes())
    fleet = json.loads((fleet_dir / 'fleet.json').read_bytes())
    fleet['_dir'] = str(fleet_dir)
    chain_id = fleet['chain_id']
    entries = {e['name']: e for e in json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())['entries']}
    funder = keystore_account(entries, funder_label, chain_id)
    sweepers = [keystore_account(entries, label, chain_id) for label in sweep_from]
    started = now_utc()
    gateway = Gateway(cli, env, out / 'gateway.log', port)
    show(f'== the gateway relays to {gateway.endpoint} at {gateway.url}')
    try:
        rpc = Rpc(gateway.url, settings.connections)
        signer = Signer(cli, env, out / 'scratch')
        load = Load(rpc, signer, env, cli, fleet, out, settings, funder, validator, config, show=show,
                    sweepers=sweepers)
        before_view = load.status('status')
        if not supply_rule(before_view):
            raise Failed(f'the supply rule fails before the run at height {before_view["height"]}')
        (out / 'status-before.json').write_text(json.dumps(before_view, indent=2) + '\n')
        before = status_reading(before_view)
        show(f'== the ledger starts at height {before["height"]}')
        try:
            # Warm-up: funding and the priming spends, outside the load metrics.
            mark = time.monotonic()
            load.fund()
            warmup = {'funding_s': round(time.monotonic() - mark, 1)}
            mark = time.monotonic()
            if load.sweepers:
                load.sync(load.sweepers)
            load.first_spends()
            warmup['priming_s'] = round(time.monotonic() - mark, 1)
            show(f'== warm-up: {warmup}')
            stop, sweep_error = threading.Event(), []

            def sweeping():
                while not stop.wait(settings.sweep_interval):
                    try:
                        returned = load.sweep(settings.sweep_keep)
                        load.sweeps.append({'height': load.tracker.head, 'returned_udrt': returned})
                        show(f'== rewards swept to {load.funder.label}: {returned}')
                    except (Failed, KeyError, ValueError) as error:
                        sweep_error.append(str(error))
                        return
            sweep_thread = threading.Thread(target=sweeping, daemon=True) if load.sweepers else None
            if sweep_thread:
                sweep_thread.start()
            gov_thread, gov_error = None, []
            if settings.governance:
                def governance():
                    try:
                        load.run_governance()
                    except (Failed, KeyError, ValueError) as error:
                        gov_error.append(error)
                gov_thread = threading.Thread(target=governance, daemon=True)
                gov_thread.start()
            window = load.window()
            if sweep_thread:
                stop.set()
                sweep_thread.join(settings.block_time * 60)
                if sweep_error:
                    raise Failed(f'the reward sweep failed: {sweep_error[0]}')
            if gov_thread:
                # With the vote, this waits for the deposit period to close.
                gov_thread.join(None if settings.governance_vote else settings.block_time * 200)
                if gov_thread.is_alive():
                    gov_error.append(Failed('the governance steps did not finish'))
                if gov_error:
                    show(f'== governance: {gov_error[0]}')
                    load.governance = dict(load.governance, error=str(gov_error[0]))
        finally:
            rows_bytes = rows_jsonl(load.rows)
            (out / 'transactions.jsonl').write_bytes(rows_bytes)
        committed = [r for r in load.rows if r['status'] == 'committed']
        last = max([r['block_height'] or r['receipt_seen_height'] for r in committed] or [before['height']])
        while True:
            after_view = load.status('status')
            if int(after_view['height']) >= last + 2:
                break
            time.sleep(2)
        (out / 'status-after.json').write_text(json.dumps(after_view, indent=2) + '\n')
        after = status_reading(after_view)
        run_rows = [r for r in load.rows if r['phase'] == 'run' and r['submit_height'] is not None]
        low = min([r['submit_height'] for r in run_rows] or [before['height']])
        high = max([r['block_height'] or 0 for r in run_rows] or [low]) + 1
        block_times = {str(h): t for h, t in load.block_times(low, high).items()}
        ledger_record = ledger(load.rows, before['height'], load.creation_fee)
        (out / 'ledger.json').write_text(json.dumps(ledger_record, indent=2) + '\n')
        # The accounts the run created, for the reconciliation's account check.
        (out / 'accounts.json').write_text(json.dumps({'addresses': created_addresses(load.rows, load.wallets)},
                                                      indent=2) + '\n')
        fleet_record = {'wallets': Counter(w['role'] for w in fleet['wallets']),
                        'addresses': [w['address'] for w in fleet['wallets']], 'funder': funder['address']}
        window_record = {'duration_s': round(window['ended'] - window['started'], 1),
                         'skipped_launches': window['skipped_launches'], 'warmup': warmup,
                         'sweeps': load.sweeps,
                         'gateway_endpoint': gateway.endpoint,
                         'read_ms': {label: distribution(values) for label, values in sorted(rpc.latency.items())}}
        settings_record = {'rate': settings.rate, 'duration': settings.duration, 'mix': settings.mix,
                           'connections': settings.connections, 'batch': settings.batch, 'signers': settings.signers,
                           'expiry_blocks': settings.expiry_blocks, 'fund_drt': settings.fund_drt,
                           'fund_dgt': settings.fund_dgt, 'governance': settings.governance,
                           'governance_vote': settings.governance_vote, 'seed': settings.seed,
                           'sweep_from': list(sweep_from), 'sweep_interval': settings.sweep_interval,
                           'distributors': sum(w['role'] == 'distributor' for w in fleet['wallets']),
                           'min_within_2': settings.min_within_2, 'max_refused': settings.max_refused}
        record = evidence(chain_id, settings_record, fleet_record, rows_bytes, ledger_record, before, after,
                          block_times, load.governance, window_record, started, now_utc())
        (out / 'evidence.json').write_text(json.dumps(record, indent=2) + '\n')
        shutil.rmtree(out / 'scratch', ignore_errors=True)
        return record
    finally:
        gateway.stop()


def adversary_run(cli, home, fleet_dir, out, application_config, funder_label, validator, settings,
                  native_genesis, engine_genesis, host, show=say, port=0, passphrase=None):
    """The adversarial generator (simulation plan, T4): on one funded wallet, send
    malformed transactions through the client channel and check the node refuses
    each at admission (an invalid ML-DSA-65 signature, a decode failure, a stale
    nonce and an oversized request), then a valid control is admitted and
    committed. Returns the adversary evidence record, which `adversary.verify`
    re-evaluates. Duplicates, re-signed copies and floods follow with the load
    machines."""
    import adversary
    out, fleet_dir = Path(out), Path(fleet_dir)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(home))
    if passphrase:
        env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    config = json.loads(Path(application_config).read_bytes())
    fleet = json.loads((fleet_dir / 'fleet.json').read_bytes())
    fleet['_dir'] = str(fleet_dir)
    chain_id = fleet['chain_id']
    entries = {e['name']: e for e in json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())['entries']}
    funder = keystore_account(entries, funder_label, chain_id)
    gateway = Gateway(cli, env, out / 'gateway.log', port)
    show(f'== the gateway relays to {gateway.endpoint} at {gateway.url}')
    try:
        rpc = Rpc(gateway.url, settings.connections)
        signer = Signer(cli, env, out / 'scratch')
        load = Load(rpc, signer, env, cli, fleet, out, settings, funder, validator, config, show=show)
        # A funded, primed wallet with a committed nonce read from the node.
        load.fund()
        load.first_spends()
        wallet = load.role('load')[0]
        load.refresh_profile()
        load.sync([wallet])
        actions = actions_for('send_drt', recipient_hex=funder['account_id'])

        def signed_at(nonce):
            """A validly signed transport at `nonce`, leaving the tracked nonce as it was."""
            keep = wallet.nonce
            wallet.nonce = nonce
            try:
                return load.sign(new_row('adversary', 'base', wallet), wallet, actions)[1]
            finally:
                wallet.nonce = keep

        def submit(name, tx, expected_reason):
            row = new_row('adversary', name, wallet)
            broadcast(rpc, [(row, tx)], name)
            # A refused transaction is never block-included, so it cannot commit.
            outcome = {'name': name, 'code': row.get('code'), 'committed': False,
                       'log': row.get('log') or '', 'expected_reason': expected_reason}
            show(f'== {name}: code {outcome["code"]}, log {outcome["log"][:120]!r}')
            return outcome

        base_tx = signed_at(wallet.nonce)
        cases = [
            submit('invalid_signature', adversary.corrupt_signature(base_tx), 'invalid ordinary signature'),
            submit('decode_failure', adversary.corrupt_encoding(base_tx), 'Malformed ordinary envelope'),
            submit('oversized', adversary.oversized_envelope(300_000), 'too large'),
            submit('stale_nonce', signed_at(0), 'Ordinary spending nonce is stale'),
        ]
        # The positive control, last: a valid transaction that must commit.
        control = load.online(wallet, ('send', '--to', funder['address'], '--amount', '1', '--token', 'drt'),
                              phase='adversary', kind='positive_control')
        committed = False
        if control['status'] == 'admitted':
            load.follow(control)
            committed = True
        cases.append({'name': 'positive_control', 'code': control.get('code'),
                      'committed': committed, 'log': control.get('log') or ''})
        show(f'== positive_control: code {control.get("code")}, committed {committed}')
        record = adversary.build_run(chain_id, native_genesis, engine_genesis, cases, host)
        (out / 'evidence.json').write_text(json.dumps(record, indent=2) + '\n')
        return record
    finally:
        gateway.stop()


def report(record, show=say):
    s = record['summary']
    show(json.dumps({'load': {'status': record['status'], 'by_status': s['by_status'], 'run_by_kind': s['run_by_kind'],
                              'inclusion': {k: s['inclusion'][k] for k in ('samples', 'share_within_2', 'histogram')},
                              'throughput': s['throughput'], 'block_interval_s': s['block_interval_s'],
                              'response_ms': s['response_ms'], 'governance': record['governance']}}, indent=2))
    for row in record['checks']:
        show(f'{row["status"]:<5} {row["check"]}: {row["detail"]}')
    show(f'== load run: {record["status"]} (evidence sha256 {record["sha256"]})')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    f = commands.add_parser('fleet')
    f.add_argument('--cli', type=Path, required=True)
    f.add_argument('--chain-id', required=True)
    f.add_argument('--out', type=Path, required=True)
    f.add_argument('--wallets', type=int, required=True)
    f.add_argument('--voters', type=int, default=1)
    f.add_argument('--spares', type=int, default=0)
    f.add_argument('--distributors', type=int, default=0, help='fan the funding out (25 for 1,000 wallets)')
    f.add_argument('--workers', type=int, default=4)
    r = commands.add_parser('run')
    r.add_argument('--cli', type=Path, required=True)
    r.add_argument('--home', type=Path, required=True, help='the CLI home with the pinned chain and the funder')
    r.add_argument('--fleet', type=Path, required=True)
    r.add_argument('--out', type=Path, required=True)
    r.add_argument('--application-config', type=Path, required=True)
    r.add_argument('--funder', required=True)
    r.add_argument('--validator', required=True)
    r.add_argument('--rate', type=float, default=1.0)
    r.add_argument('--duration', type=float, default=120)
    r.add_argument('--mix', default=DEFAULT_MIX)
    r.add_argument('--connections', type=int, default=2)
    r.add_argument('--batch', type=int, default=MAX_BATCH)
    r.add_argument('--signers', type=int, default=4)
    r.add_argument('--expiry-blocks', type=int, default=20)
    r.add_argument('--fund-drt', type=int, default=200, help='DRT per wallet')
    r.add_argument('--fund-dgt', type=int, default=500, help='DGT per wallet')
    r.add_argument('--governance', action='store_true', help='propose and deposit beside the load')
    r.add_argument('--governance-vote', action='store_true', help='then wait for the vote to open, and vote')
    r.add_argument('--sweep-from', action='append', default=[],
                   help='a keystore wallet (an operator, a staker) whose rewards go back to the funder')
    r.add_argument('--sweep-interval', type=float, default=3600)
    r.add_argument('--sweep-keep', type=int, default=100, help='DRT each sweeper keeps')
    r.add_argument('--seed', type=int, default=1)
    r.add_argument('--min-within-2', type=float, default=0.99)
    r.add_argument('--max-refused', type=int)
    r.add_argument('--port', type=int, default=0)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'fleet':
            make_fleet(args.cli, args.chain_id, args.out, args.wallets, args.voters, args.spares, args.distributors,
                       args.workers)
        elif args.command == 'run':
            if not 2 <= args.batch <= MAX_BATCH:
                raise Failed(f'--batch is 2 to {MAX_BATCH}: the head height rides in each request')
            settings = Settings(rate=args.rate, duration=args.duration, mix=parse_mix(args.mix),
                                connections=args.connections, batch=args.batch, signers=args.signers,
                                expiry_blocks=args.expiry_blocks, fund_drt=args.fund_drt * UNIT,
                                fund_dgt=args.fund_dgt * UNIT, governance=args.governance or args.governance_vote,
                                governance_vote=args.governance_vote, seed=args.seed,
                                sweep_interval=args.sweep_interval, sweep_keep=args.sweep_keep * UNIT,
                                min_within_2=args.min_within_2, max_refused=args.max_refused)
            record = run_load(args.cli, args.home, args.fleet, args.out, args.application_config, args.funder,
                              args.validator, settings, port=args.port, sweep_from=args.sweep_from)
            report(record)
            return 0 if record['status'] == PASS else 1
        else:
            record = json.loads(args.evidence.read_bytes())
            status = verify(record, (args.evidence.parent / 'transactions.jsonl').read_bytes())
            print(status)
            return 0 if status == PASS else 1
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'load: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
