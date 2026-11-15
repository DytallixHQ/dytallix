#!/usr/bin/env python3
"""The attack campaign's recovery family (simulation plan, statistical
campaign; launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json:
replayed and wrong-key recovery transactions): each type at least 3,000
times on the staging hosts, with zero successes.

  account_recovery.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                             --engine-genesis ENGINE_GENESIS

`staging_network.py recovery-campaign` runs it on the staging network: a
fresh fleet funded from the ecosystem growth account. Its targets each
enroll three fresh guardian keys, and a separate sponsor pays for every
recovery transaction (the recovering account never pays). Every
transaction is made by the release CLI's recovery commands (prepare, sign
with each key, assemble, sponsor, submit) and goes to the public node
through the client channel. Each type and what it sends:
  - replayed_recovery: a target's committed enrollment again: either the
    committed transaction byte for byte (the CLI signs deterministically,
    so signing it again makes the same bytes), checked at admission
    directly (check_tx), since the engine's cache answers bytes it has seen
    before the node can; or its operation sponsored again with the
    sponsor's current nonce (a new authorization for the same operation),
    sent;
  - wrong_key_recovery: a recovery start for a target, signed in the
    guardian role by two keys of which one at least is no guardian: two
    outsiders, a guardian and an outsider, or a guardian and the account's
    own active key. The operation is the one the CLI prepares from the
    target's view with those keys named as its guardians; the operation
    itself names no guardian (only the policy version and the recovery
    sequence it was prepared at), so only its signers are wrong.
The node verifies every signature, then checks a recovery against the
committed state at admission (CheckTx) exactly as the block would, so both
types are refused there, uncharged, with the rule as the log. An attempt
counts as refused only when the node gave the type's reason, its row's
fault is the type's (for a replay: the committed operation, its receipt a
success, the attempt not expired, and either the committed bytes, as the
node's engine hash names them, or a new sponsorship; for a wrong key: the signers
against the policy and active key read from the chain, the authority the
operation was prepared at the chain's), and the target's recovery state and
the sponsor's nonce and liquid DRT read before and after it are unchanged.
Admission, or any change, is a success (a defect). A refusal for another
reason is counted apart and fails the type (the attempt did not test what
it names). An attempt the channel could not make is counted apart and made
again.

The attempts go in rounds, one per target at a time. The positive
controls: each target's enrollment, the replays' source, and after each
type a fresh wallet's enrollment, each followed to its sponsor receipt,
verified against the chain's application hash, which must show the
operation a success and the policy enrolled; the height rises from control
to control. A replay must reach the node before its operation's submission
expiry (after it the node refuses it as expired, and prunes its receipt),
so the replays stop with an error when too few blocks are left.

verify recomputes every outcome, count, status and digest from the rows,
which name no address; the evidence binds the genesis files.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import attacks  # noqa: E402
import load  # noqa: E402

SCHEMA = attacks.SCHEMA
FAMILY = 'recovery'
PASS, FAIL = attacks.PASS, attacks.FAIL
TARGET = 3000
OUTCOMES = attacks.OUTCOMES
REPLAYED, WRONG_KEY = 'replayed_recovery', 'wrong_key_recovery'
# The node's words for each type (recovery_fees.rs, recovery.rs).
REASONS = {
    REPLAYED: 'Consumed sponsorship or successful operation',
    WRONG_KEY: 'signer is not a current guardian',
}
REPLAYS = ('committed', 'responsored')
# Each wrong-key shape: how many of its two signers are guardians, and
# whether the other is the account's active key.
SHAPES = {'outsiders': (0, False), 'with_guardian': (1, False), 'active_key': (1, True)}
# What a target's recovery view holds that an attempt must not change.
ACCOUNT = ('status', 'active_key', 'active_generation', 'spending_nonce', 'sponsor_nonce', 'policy', 'policy_version',
           'recovery_sequence', 'policy_change_sequence', 'pending_recovery', 'pending_policy')
TRIES = 3
RECEIPT_TIMEOUT = 120
CONTROL_TIMEOUT = 120
# Blocks a replay round leaves before its committed operations expire.
MARGIN = 20
# Blocks a start may wait for admission (it never commits).
START_LIFETIME = 100


class Failed(Exception):
    pass


class Unavailable(Exception):
    """The CLI could not reach the node (no answer)."""


digest, rows_bytes = attacks.digest, attacks.rows_bytes


def record_digest(value):
    return digest(attacks.canonical(value))


def fingerprint(identity):
    """A key's digest, so a row names keys without their bytes."""
    return digest(bytes(identity['public_key']))


def identity(key_file):
    key = json.loads(Path(key_file).read_bytes())
    return {'algorithm': key['algorithm'], 'public_key': key['public_key']}


def in_key_order(key_files):
    """Key files in the order a policy must list their keys (strictly
    ascending, as the node orders key identities)."""
    return sorted(key_files, key=lambda f: (identity(f)['algorithm'], identity(f)['public_key']))


def account_digest(view):
    return record_digest({k: view.get(k) for k in ACCOUNT})


def hex_id(value):
    """A 32-byte ID as the CLI prints it, from a receipt's byte array."""
    return bytes(value).hex() if isinstance(value, list) else str(value).lower()


# Outcomes

def as_named(row):
    """Whether an attempt carries exactly its type's fault."""
    fault, height = row.get('fault') or {}, row.get('height')
    if not isinstance(height, int):
        return False
    if row['type'] == REPLAYED:
        committed = fault.get('committed') or {}
        same = fault.get('authorization_id') == committed.get('authorization_id')
        live = (committed.get('success') is True and fault.get('operation_id') == committed.get('operation_id')
                and height + 1 < committed.get('retained_until', 0) and height + 1 < fault.get('sponsor_expiry', 0))
        if row['variant'] == 'committed':
            # The committed bytes themselves, as the node hashed them.
            return (live and same and row.get('route') == 'check' and fault.get('tx_sha256') is not None
                    and fault.get('tx_sha256') == committed.get('engine_hash')
                    and fault.get('envelope_sha256') == committed.get('envelope_sha256'))
        return (live and row['variant'] == 'responsored' and not same and row.get('route') == 'submit'
                and fault.get('sponsor_nonce_signed') == row['before']['sponsor_nonce']
                and fault.get('envelope_sha256') != committed.get('envelope_sha256'))
    if row['type'] == WRONG_KEY:
        shape = SHAPES.get(row['variant'])
        signers, guardians, active = fault.get('signers') or [], fault.get('guardians') or [], fault.get('active')
        authority = fault.get('authority') or {}
        return (shape is not None and len(signers) == 2 and len(set(signers)) == 2 and len(set(guardians)) == 3
                and sum(s in guardians for s in signers) == shape[0] and (active in signers) == shape[1]
                and active not in guardians and fault.get('replacement') not in [*guardians, active, *signers]
                and authority.get('status') == 'Normal' and authority.get('pending') is False)
    return False


def changed(row):
    return row.get('before') != row.get('after')


def classify(row):
    """An attempt's outcome from the node's answer, its fault and the reads
    around it."""
    if row['type'] not in REASONS:
        raise Failed(f'{row["type"]!r} is not an approved recovery attack type')
    code = row.get('code')
    if not isinstance(code, int) or isinstance(code, bool):
        return 'transport'
    if code == 0 or changed(row):
        return 'admitted'
    if not as_named(row):
        return 'refused_other'
    return 'refused' if REASONS[row['type']] in (row.get('log') or '') else 'refused_other'


def control_ok(row):
    """An enrollment admitted, committed a success by its verified receipt,
    and the policy then on the chain."""
    receipt = row.get('receipt') or {}
    return (row.get('code') == 0 and receipt.get('success') is True and row.get('enrolled') is True
            and receipt.get('operation_id') == row.get('operation_id')
            and receipt.get('authorization_id') == row.get('authorization_id') and isinstance(row.get('height'), int))


class Campaign:
    """Each type's attempts against the node, through the desk."""

    def __init__(self, desk, attempts, show=print, clock=time.monotonic, sleep=time.sleep):
        self.desk, self.attempts, self.show, self.clock, self.sleep = desk, attempts, show, clock, sleep
        self.rows, self.height = [], None

    def control(self, what, label):
        """The positive control: one block more than at the last control,
        then the wallet's enrollment, followed to its receipt."""
        deadline = self.clock() + CONTROL_TIMEOUT
        height = self.desk.head()
        while self.height is not None and height <= self.height:
            if self.clock() > deadline:
                raise Failed(f'the chain did not advance past height {self.height}')
            self.sleep(1)
            height = self.desk.head()
        row = {'type': 'control', 'what': what, 'height': height}
        for _ in range(TRIES):
            try:
                row.update(self.desk.enroll(label))
                break
            except Unavailable as error:
                row['error'] = str(error)[:200]
        self.rows.append(row)
        self.height = height
        if not control_ok(row):
            self.show(f'== the positive control ({what}) did not commit an enrollment: '
                      f'{ {k: row.get(k) for k in ("code", "log", "receipt", "enrolled", "error")} }')
        return control_ok(row)

    def round(self, kind, ks):
        """Attempts ks, one a target at once, the targets read before and
        after."""
        targets = self.desk.roles['targets']
        jobs = list(zip(ks, targets))
        if kind == REPLAYED:
            head = self.desk.head()
            last = min(min(c['retained_until'], c['expiry']) for c in self.desk.committed.values())
            if head + 1 + MARGIN >= last:
                raise Failed(f'the committed enrollments expire at height {last}, too soon for more replays '
                             f'(height {head}); enroll more targets')
        head, before, views = self.desk.ready(kind, [t for _, t in jobs])
        with ThreadPoolExecutor(max_workers=len(jobs)) as pool:
            made = list(pool.map(lambda job: self.desk.attempt(kind, job[0], job[1], views), jobs))
        after = self.desk.states([t for _, t in jobs])
        outcomes = []
        for (k, target), found in zip(jobs, made):
            row = {'type': kind, 'k': k, 'target': targets.index(target), 'height': head, **found,
                   'before': before[target], 'after': after[target]}
            row['outcome'] = classify(row)
            self.rows.append(row)
            outcomes.append((k, row['outcome']))
        return outcomes

    def run_type(self, kind):
        self.show(f'== {kind}: {self.attempts} attempts, {len(self.desk.roles["targets"])} targets at a time')
        pending, tries = list(range(self.attempts)), {}
        while pending:
            batch, pending = pending[:len(self.desk.roles['targets'])], pending[len(self.desk.roles['targets']):]
            for k, outcome in self.round(kind, batch):
                if outcome == 'transport':
                    tries[k] = tries.get(k, 0) + 1
                    if tries[k] >= TRIES:
                        raise Failed(f'{kind} attempt {k} could not be made {TRIES} times running')
                    pending.append(k)
        mine = [r for r in self.rows if r['type'] == kind]
        counts = {o: sum(r['outcome'] == o for r in mine) for o in OUTCOMES if o != 'transport'}
        said = next((r['log'] for r in reversed(mine) if 'log' in r), None)
        self.show(f'== {kind}: {counts}; the node said {said!r}')

    def run(self, kinds):
        for kind in kinds:
            if kind not in REASONS:
                raise Failed(f'{kind!r} is not an approved recovery attack type')
        if len(self.desk.roles['controls']) < len(kinds):
            raise Failed('the campaign needs one control wallet a type')
        self.show(f'== {len(self.desk.roles["targets"])} targets enroll their guardians')
        enrolled = [self.control('enroll', label) for label in self.desk.roles['targets']]
        if not all(enrolled):
            # Nothing to replay or start against; the record fails on its controls.
            self.show('== no attempt made, since a target did not enroll')
            return self.rows
        self.desk.keep_committed()
        for kind, label in zip(kinds, self.desk.roles['controls']):
            self.run_type(kind)
            self.control(f'after {kind}', label)
        return self.rows


def tally(rows, attempts):
    """Each type's outcome counts, first log and status: PASS once the wanted
    attempts were refused with no success and no other refusal."""
    types = {}
    for kind in REASONS:
        mine = [r for r in rows if r['type'] == kind]
        counts = {o: sum(1 for r in mine if r['outcome'] == o) for o in OUTCOMES}
        if counts['admitted'] or counts['refused_other']:
            status = FAIL
        elif not mine:
            status = 'not_run'
        else:
            status = PASS if counts['refused'] >= attempts else FAIL
        types[kind] = {'reason': REASONS[kind], 'counts': counts, 'status': status,
                       'first_log': next((r['log'] for r in mine if 'log' in r), None)}
    return types


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, materials, started, finished):
    for row in rows:
        if row['type'] != 'control' and classify(row) != row['outcome']:
            raise Failed(f'a {row["type"]} row\'s outcome does not follow from its answer')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    heights = [r.get('height') for r in controls]
    ran = [kind for kind, t in types.items() if t['status'] != 'not_run']
    checks = {
        'types': all(t['status'] in (PASS, 'not_run') for t in types.values()),
        'controls': bool(controls) and all(map(control_ok, controls))
        and sum(r['what'] == 'enroll' for r in controls) == materials['targets']
        and sorted(r['what'] for r in controls if r['what'] != 'enroll') == sorted(f'after {k}' for k in ran),
        'advancing': all(isinstance(h, int) for h in heights) and all(a < b for a, b in zip(heights, heights[1:])),
    }
    body = {'schema': SCHEMA, 'family': FAMILY, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'started': started, 'finished': finished, 'attempts_wanted': attempts, 'materials': materials,
            'types': types,
            'controls': {'made': len(controls), 'passed': sum(map(control_ok, controls))},
            'successes': sum(t['counts']['admitted'] for t in types.values()),
            'rows_sha256': digest(raw), 'checks': checks,
            'status': PASS if all(checks.values()) else FAIL}
    body['sha256'] = record_digest(body)
    return body


def verify(record, raw, genesis_bytes, engine_genesis_bytes):
    """Re-evaluates a campaign record from its rows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or record_digest(body) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['family'] != FAMILY:
        raise Failed(f'the record is for {record["family"]}, not {FAMILY}')
    if digest(raw) != record['rows_sha256']:
        raise Failed('the rows are not the ones the evidence names')
    if record['genesis'] != {'native_genesis_sha256': digest(genesis_bytes),
                             'engine_genesis_sha256': digest(engine_genesis_bytes)}:
        raise Failed('the evidence names another genesis')
    rows = [json.loads(line) for line in raw.splitlines() if line.strip()]
    again = build_record(record['chain_id'], genesis_bytes, engine_genesis_bytes, rows, record['attempts_wanted'],
                         record['materials'], record['started'], record['finished'])
    if again != record:
        raise Failed('the counts and statuses do not follow from the rows')
    return record['status']


# The node, through the release CLI and the client channel

def last_json(text):
    """The report the CLI printed last (one JSON object a line)."""
    for line in reversed((text or '').strip().splitlines()):
        try:
            value = json.loads(line)
        except ValueError:
            continue
        if isinstance(value, dict):
            return value
    return None


def envelope_digest(tx):
    return digest(base64.b64decode(json.loads(Path(tx).read_bytes())['envelope_base64']))


def wire_bytes(tx, engine_hash):
    """The bytes the CLI sent for a transaction file (sdk recovery.rs
    transaction: a compact JSON object), found by the engine hash the node
    answered with; None if neither field order gives it."""
    envelope = json.loads(Path(tx).read_bytes())['envelope_base64']
    for value in ({'envelope_base64': envelope, 'kind': 'recovery'}, {'kind': 'recovery', 'envelope_base64': envelope}):
        raw = json.dumps(value, separators=(',', ':')).encode()
        if engine_hash and digest(raw) == engine_hash.lower():
            return raw
    return None


def fee_bounds(fee):
    """The largest gas limit the recovery profile takes, and its charge
    (the gas limit times the gas price, within the fee cap)."""
    price = int(fee['gas_price'])
    gas = min(int(fee['max_transaction_gas']), int(fee['max_fee_cap']) // price)
    if gas < int(fee['minimum_gas']):
        raise Failed(f'the recovery profile allows no gas limit: {fee}')
    return gas, gas * price


class Desk:
    """The campaign's wallets on the node. `roles` names the targets (each
    enrolls three fresh guardian keys, then takes the attempts), the
    control wallets (each enrolls once, after a type) and the sponsor,
    which pays every recovery. The outsiders (two fresh keys, no guardian)
    and the replacement (a fresh key every start names) are made here too.
    Every write is made by the release CLI from the views it captured."""

    def __init__(self, rpc, cli, env, endpoint, out, wallets, roles, show=load.say, sleep=time.sleep,
                 clock=time.monotonic):
        self.rpc, self.cli, self.env, self.endpoint = rpc, cli, env, str(endpoint)
        self.out, self.by_label, self.roles = Path(out), wallets, roles
        self.show, self.sleep, self.clock = show, sleep, clock
        self.keys = self.out / 'keys'
        self.guardians, self.outsiders, self.replacement = {}, [], None
        self.committed, self.enrolled, self.bounds, self.lifetime = {}, {}, None, None
        self.made = iter(range(1, 10 ** 9))

    def prepare(self):
        """The fresh keys, made with the release's CLI, and the fee bounds."""
        enrolling = [*self.roles['targets'], *self.roles['controls']]
        labels = [f'{w}-guardian-{n}' for w in enrolling for n in (1, 2, 3)] + ['outsider-1', 'outsider-2',
                                                                                 'replacement']
        self.keys.mkdir(parents=True, mode=0o700)
        scratch = Path(tempfile.mkdtemp(prefix='keys-', dir=self.out))
        passphrase = scratch / 'passphrase'
        load.private_file(passphrase, (digest(os.urandom(32)) + '\n').encode())
        self.show(f'== {len(labels)} fresh keys made with dytallix wallet create and export')
        try:
            with ThreadPoolExecutor(max_workers=4) as pool:
                list(pool.map(lambda label: load.make_wallet(self.cli, label, self.keys, scratch, passphrase), labels))
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
        self.guardians = {w: in_key_order(self.keys / f'{w}-guardian-{n}.json' for n in (1, 2, 3)) for w in enrolling}
        self.outsiders = [self.keys / 'outsider-1.json', self.keys / 'outsider-2.json']
        self.replacement = self.keys / 'replacement.json'
        view = self.account(self.roles['sponsor'])
        self.bounds, self.lifetime = fee_bounds(view['fee']), int(view['timing']['submission_lifetime'])
        liquid = int(self.liquid(self.roles['sponsor']))
        if liquid < 2 * self.bounds[1]:
            raise Failed(f'the sponsor holds {liquid} udrt, under two charges of {self.bounds[1]}')
        self.show(f'== recoveries sponsored by {self.roles["sponsor"]}: gas limit {self.bounds[0]}, '
                  f'up to {self.bounds[1]} udrt each')

    # Reads

    def head(self):
        return load.head_height(self.rpc.one('head', 'abci_info', {}))

    def account(self, label):
        return load.query(self.rpc, 'recovery', f'/recovery/account/{self.by_label[label].account_id}')

    def liquid(self, label):
        return str(load.query(self.rpc, 'accounts', f'/account/{self.by_label[label].address}')['liquid_udrt'])

    def states(self, labels):
        """What each target's attempt must leave as it was: its recovery
        state, and the sponsor's nonce and liquid DRT."""
        sponsor = self.roles['sponsor']
        views = load.queries(self.rpc, 'recovery',
                             [f'/recovery/account/{self.by_label[w].account_id}' for w in [*labels, sponsor]])
        if any(v is None for v in views):
            raise Failed('a campaign wallet has no recovery record')
        paying = {'sponsor_nonce': int(views[-1]['sponsor_nonce']), 'sponsor_liquid_udrt': self.liquid(sponsor)}
        return {w: {'account': account_digest(v), **paying} for w, v in zip(labels, views)}

    # The CLI

    def work(self, name):
        path = self.out / 'work' / f'{next(self.made):06}-{name}'
        path.mkdir(parents=True)
        return path

    def tool(self, *args, node=False):
        """One recovery command's report. A command that reaches the node
        and gets no report is Unavailable (made again); an offline one that
        fails is the campaign's own fault."""
        command = [str(self.cli), 'recovery', *map(str, args)]
        if node:
            with self.rpc.slots:
                result = subprocess.run(command, capture_output=True, text=True, env=self.env)
        else:
            result = subprocess.run(command, capture_output=True, text=True, env=self.env)
        report = last_json(result.stdout)
        if result.returncode != 0 or report is None:
            message = (result.stderr.strip() or result.stdout.strip())[-300:]
            raise (Unavailable if node else Failed)(f'dytallix recovery {args[0]}: {message}')
        return report

    def view(self, label, path):
        self.tool('query', '--endpoint', self.endpoint, '--account-id', self.by_label[label].account_id,
                  '--output', path, node=True)
        return json.loads(Path(path).read_bytes())

    def operation(self, view_path, request, expiry, work):
        request_path, out = work / 'request.json', work / 'operation.json'
        request_path.write_text(json.dumps(request))
        self.tool('prepare', '--view', view_path, '--request', request_path, '--expiry-height', expiry, '--output', out)
        return out

    def sign(self, operation, role, key_file, work, name):
        out = work / f'{name}.sig.json'
        self.tool('sign', '--operation', operation, '--role', role, '--key-file', key_file, '--output', out)
        return out

    def sponsor(self, signed, target_view, sponsor_view, expiry, work):
        """The sponsor's signature over `signed`: (transaction file, its
        operation and authorization IDs)."""
        out = work / 'transaction.json'
        sponsor = self.by_label[self.roles['sponsor']]
        report = self.tool('sponsor', '--signed', signed, '--target-view', target_view, '--sponsor-view', sponsor_view,
                           '--key-file', sponsor.key_file, '--gas-limit', self.bounds[0], '--maximum-charge-udrt',
                           self.bounds[1], '--expiry-height', expiry, '--output', out)
        return out, report['operation_id'], report['authorization_id']

    def assemble(self, operation, signatures, work):
        out = work / 'signed.json'
        self.tool('assemble', '--operation', operation, *(a for s in signatures for a in ('--signature', s)),
                  '--output', out)
        return out

    def submit(self, transaction):
        """The node's admission answer. With no answer, Unavailable."""
        command = [str(self.cli), 'recovery', 'submit', '--endpoint', self.endpoint, '--transaction', str(transaction)]
        with self.rpc.slots:
            result = subprocess.run(command, capture_output=True, text=True, env=self.env)
        report = last_json(result.stdout)
        if not report or not isinstance(report.get('check_tx'), dict):
            raise Unavailable(f'dytallix recovery submit: {(result.stderr.strip() or result.stdout.strip())[-300:]}')
        check = report['check_tx']
        answer = {'code': int(check['code']), 'log': (check.get('log') or '')[:400]}
        if check.get('engine_hash'):
            answer['engine_hash'] = check['engine_hash'].lower()
        return answer

    def check(self, raw):
        """Admission's answer to exact bytes, asked directly (check_tx):
        the engine's cache answers bytes it has seen before the node can."""
        try:
            result = self.rpc.one('check', 'check_tx', {'tx': base64.b64encode(raw).decode()})
        except load.Failed as error:
            raise Unavailable(f'check_tx: {error}'[:300])
        return {'code': int(result.get('code') or 0), 'log': (result.get('log') or '')[:400]}

    def receipt(self, transaction):
        """The transaction's sponsor receipt, verified against the chain's
        application hash; None if none appears in time. A read with no
        answer is read again: the transaction was already sent."""
        deadline = self.clock() + RECEIPT_TIMEOUT
        while True:
            try:
                report = self.tool('receipt', '--endpoint', self.endpoint, '--transaction', transaction, node=True)
            except Unavailable:
                report = {}
            if report.get('status') == 'committed_receipt_verified':
                found = report['receipt']
                return {'success': found['success'] is True, 'block_height': int(found['block_height']),
                        'retained_until': int(found['retained_until']), 'gas_used': int(found['gas_used']),
                        'settled_fee': str(found['settled_fee']), 'operation_id': hex_id(found['operation_id']),
                        'authorization_id': hex_id(found['sponsor_authorization_id'])}
            if self.clock() > deadline:
                return None
            self.sleep(1)

    # Writes

    def enroll(self, label):
        """The wallet's enrollment of its three guardian keys, sponsored,
        followed to its receipt: the control row's facts."""
        work = self.work(f'enroll-{label}')
        target_view, sponsor_view = work / 'target-view.json', work / 'sponsor-view.json'
        view = self.view(label, target_view)
        sponsoring = self.view(self.roles['sponsor'], sponsor_view)
        guardians = self.guardians[label]
        policy = {'threshold': 2, 'guardians': [{'key': identity(g), 'control_group': f'guardian-{n}'}
                                                for n, g in enumerate(guardians, 1)]}
        expiry = int(view['context']['height']) + self.lifetime
        operation = self.operation(target_view, {'action': 'enroll', 'policy': policy}, expiry, work)
        signatures = [self.sign(operation, 'operation', self.by_label[label].key_file, work, 'active')]
        signatures += [self.sign(operation, 'possession', g, work, f'guardian-{n}') for n, g in enumerate(guardians, 1)]
        signed = self.assemble(operation, signatures, work)
        transaction, operation_id, authorization_id = self.sponsor(signed, target_view, sponsor_view, expiry, work)
        answer = self.submit(transaction)
        receipt = self.receipt(transaction) if answer['code'] == 0 else None
        after = self.account(label) if receipt else None
        enrolled = bool(after and after.get('policy')) and sorted(
            fingerprint(g['key']) for g in after['policy']['guardians']) == sorted(
            fingerprint(identity(g)) for g in guardians)
        self.enrolled[label] = {'signed': signed, 'target_view': target_view, 'expiry': expiry,
                                'sponsor_nonce': int(sponsoring['sponsor_nonce']), 'operation_id': operation_id,
                                'authorization_id': authorization_id, 'envelope_sha256': envelope_digest(transaction),
                                'engine_hash': answer.get('engine_hash'),
                                'wire': wire_bytes(transaction, answer.get('engine_hash')), 'receipt': receipt}
        return {'code': answer['code'], 'log': answer['log'], 'operation_id': operation_id,
                'authorization_id': authorization_id, 'receipt': receipt, 'enrolled': enrolled}

    def keep_committed(self):
        """The targets' committed enrollments, the replays' source."""
        for label in self.roles['targets']:
            e = self.enrolled[label]
            if e['wire'] is None:
                raise Failed(f'the bytes of {label}\'s enrollment are not the ones the node hashed '
                             f'({e["engine_hash"]})')
            self.committed[label] = {**e, 'success': e['receipt']['success'],
                                     'block_height': e['receipt']['block_height'],
                                     'retained_until': e['receipt']['retained_until']}

    def ready(self, kind, labels):
        """Before a round: the height, the targets' states and the views the
        round's attempts are made from."""
        head = self.head()
        before = self.states(labels)
        views = {}
        if kind == WRONG_KEY:
            work = self.work('round')
            views['sponsor'] = work / 'sponsor-view.json'
            self.view(self.roles['sponsor'], views['sponsor'])
            for label in labels:
                views[label] = work / f'{label}-view.json'
                self.view(label, views[label])
        return head, before, views

    def attempt(self, kind, k, label, views):
        """Attempt k on the target: its variant, fault and the node's answer
        (or the error that kept the answer away)."""
        work = self.work(f'{kind}-{k}')
        try:
            if kind == REPLAYED:
                variant, fault, (route, payload) = self.replay(k, label, work)
            else:
                variant, fault, (route, payload) = self.start(k, label, views, work)
            found = {'variant': variant, 'route': route, 'fault': fault}
            try:
                answer = self.check(payload) if route == 'check' else self.submit(payload)
                found.update(code=answer['code'], log=answer['log'])
            except Unavailable as error:
                found['error'] = str(error)[:200]
            return found
        finally:
            shutil.rmtree(work, ignore_errors=True)

    def replay(self, k, label, work):
        committed = self.committed[label]
        variant = REPLAYS[k % len(REPLAYS)]
        kept = {key: committed[key] for key in ('operation_id', 'authorization_id', 'envelope_sha256', 'engine_hash',
                                                 'success', 'block_height', 'retained_until')}
        if variant == 'committed':
            raw = committed['wire']
            fault = {'operation_id': committed['operation_id'], 'authorization_id': committed['authorization_id'],
                     'envelope_sha256': committed['envelope_sha256'], 'tx_sha256': digest(raw),
                     'sponsor_nonce_signed': committed['sponsor_nonce'], 'sponsor_expiry': committed['expiry'],
                     'committed': kept}
            return variant, fault, ('check', raw)
        sponsor_view = work / 'sponsor-view.json'
        view = self.view(self.roles['sponsor'], sponsor_view)
        expiry, nonce = int(view['context']['height']) + MARGIN + k, int(view['sponsor_nonce'])
        transaction, operation_id, authorization_id = self.sponsor(committed['signed'], committed['target_view'],
                                                                   sponsor_view, expiry, work)
        fault = {'operation_id': operation_id, 'authorization_id': authorization_id,
                 'envelope_sha256': envelope_digest(transaction), 'sponsor_nonce_signed': nonce,
                 'sponsor_expiry': expiry, 'committed': kept}
        return variant, fault, ('submit', transaction)

    def start(self, k, label, views, work):
        """A recovery start for the target, prepared from its view with the
        shape's two signers named as guardians, and signed by them."""
        variant = list(SHAPES)[k % len(SHAPES)]
        view = json.loads(views[label].read_bytes())
        g, (o1, o2), active = self.guardians[label], self.outsiders, self.by_label[label].key_file
        named, signers = {'outsiders': ([o1, o2, g[2]], [o1, o2]), 'with_guardian': ([g[0], o1, g[2]], [g[0], o1]),
                          'active_key': ([g[0], active, g[2]], [g[0], active])}[variant]
        named_view = work / 'named-view.json'
        named_view.write_text(json.dumps({**view, 'policy': {
            'threshold': view['policy']['threshold'],
            'guardians': [{'key': identity(f), 'control_group': f'named-{n}'}
                          for n, f in enumerate(in_key_order(named), 1)]}}))
        request = {'action': 'start', 'replacement': identity(self.replacement),
                   'request_id': digest(f'{WRONG_KEY} {label} {k}'.encode())}
        expiry = int(view['context']['height']) + min(self.lifetime, START_LIFETIME)
        operation = self.operation(named_view, request, expiry, work)
        signatures = [self.sign(operation, 'operation', f, work, f'signer-{n}') for n, f in enumerate(signers, 1)]
        signatures.append(self.sign(operation, 'possession', self.replacement, work, 'replacement'))
        signed = self.assemble(operation, signatures, work)
        transaction, operation_id, _ = self.sponsor(signed, views[label], views['sponsor'], expiry, work)
        fault = {'operation_id': operation_id, 'signers': [fingerprint(identity(f)) for f in signers],
                 'guardians': [fingerprint(x['key']) for x in view['policy']['guardians']],
                 'active': fingerprint(view['active_key']), 'replacement': fingerprint(identity(self.replacement)),
                 'authority': {'status': view['status'], 'pending': view['pending_recovery'] is not None,
                               'policy_version': str(view['policy_version']),
                               'recovery_sequence': str(view['recovery_sequence'])}}
        return variant, fault, ('submit', transaction)


def materials_of(desk):
    return {'targets': len(desk.roles['targets']), 'gas_limit': desk.bounds[0],
            'maximum_charge_udrt': str(desk.bounds[1]), 'submission_lifetime': desk.lifetime,
            'committed': [{k: c[k] for k in ('operation_id', 'authorization_id', 'engine_hash', 'block_height',
                                             'retained_until')} for c in desk.committed.values()],
            'replays': list(REPLAYS), 'shapes': list(SHAPES)}


def run_campaign(cli, home, fleet_dir, out, application_config, funder_label, validator, endpoint, attempts, kinds,
                 native_genesis, engine_genesis, show=load.say, passphrase=None, settings=None):
    out, fleet_dir = Path(out), Path(fleet_dir)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(home))
    if passphrase:
        env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    config = json.loads(Path(application_config).read_bytes())
    fleet = json.loads((fleet_dir / 'fleet.json').read_bytes())
    fleet['_dir'] = str(fleet_dir)
    entries = {e['name']: e for e in json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())['entries']}
    funder = load.keystore_account(entries, funder_label, fleet['chain_id'])
    settings = settings or load.Settings()
    started = load.now_utc()
    gateway = load.Gateway(cli, env, out / 'gateway.log')
    show(f'== the gateway relays to {gateway.endpoint} at {gateway.url}')
    try:
        rpc = load.Rpc(gateway.url, settings.connections)
        run = load.Load(rpc, load.Signer(cli, env, out / 'scratch'), env, cli, fleet, out, settings, funder, validator,
                        config, show=show)
        wallets = [w.label for w in run.role('load')]
        controls = len(kinds)
        if len(wallets) < controls + 2:
            raise Failed(f'the campaign needs a target, {controls} control wallets and a sponsor')
        roles = {'targets': wallets[:-controls - 1], 'controls': wallets[-controls - 1:-1], 'sponsor': wallets[-1]}
        desk = Desk(rpc, cli, env, endpoint, out, run.wallets, roles, show=show)
        # Recovery never makes a record: each wallet's comes with its first spend.
        run.fund()
        run.first_spends()
        desk.prepare()
        rows = Campaign(desk, attempts, show).run(kinds)
        record = build_record(fleet['chain_id'], native_genesis, engine_genesis, rows, attempts, materials_of(desk),
                              started, load.now_utc())
        # The rows beside the record, as results.py reads them (FILE.json, FILE.jsonl).
        (out / 'recovery.jsonl').write_bytes(rows_bytes(rows))
        (out / 'recovery.json').write_text(json.dumps(record, indent=2) + '\n')
        return record
    finally:
        gateway.stop()


def report(record, show=load.say):
    for kind, t in record['types'].items():
        show(f'== {kind}: {t["status"]} {t["counts"]}; first: {(t["first_log"] or "")[:160]!r}')
    show(f'== controls {record["controls"]}, successes {record["successes"]}, checks {record["checks"]}: '
         f'{record["status"]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    for name in ('--evidence', '--rows', '--genesis', '--engine-genesis'):
        v.add_argument(name, type=Path, required=True)
    args = parser.parse_args()
    try:
        status = verify(json.loads(args.evidence.read_bytes()), args.rows.read_bytes(), args.genesis.read_bytes(),
                        args.engine_genesis.read_bytes())
    except (Failed, KeyError, TypeError, ValueError) as error:
        print(f'account_recovery: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    sys.exit(main())
