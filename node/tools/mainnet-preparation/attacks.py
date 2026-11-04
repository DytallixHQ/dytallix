#!/usr/bin/env python3
"""The attack campaign (simulation plan, statistical campaign;
launch/approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json and
P01_T06_SIMULATION_INPUTS_2026-10-14.json): each attack type attempted at
least 3,000 times with zero successes, which bounds its success rate below 1
in 1,000 at 95% confidence (the rule of three). This is the transaction
family.

  attacks.py run --cli DYTALLIX --home HOME --fleet DIR --out DIR
                 --application-config FILE --funder WALLET --validator ID
                 --genesis NATIVE_GENESIS --engine-genesis ENGINE_GENESIS
                 [--attempts N] [--types T,...]
  attacks.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                    --engine-genesis ENGINE_GENESIS

run funds and primes the fleet's load wallets (load.py's funding), then
makes N attempts of each attack type through the client channel, each with
its own bytes:
  - invalid_signature, truncated_signature, malformed_envelope,
    non_canonical_envelope (bytes after the signature) and
    oversized_envelope: one valid transaction, never sent, mutated at a
    different byte (or by a different length) each attempt;
  - wrong_chain, wrong_network and wrong_key_signature: signed by the
    release CLI against a falsified view, as an attacker can make it sign
    (another chain ID; the account's mainnet identity; another wallet's key
    presented as the account's current key);
  - expired: valid transactions signed with an expiry two blocks ahead and
    held until the chain passes it;
  - stale_nonce (the nonce before the committed one) and overspend (more
    than any balance);
  - duplicate, resigned_copy, conflicting_nonce and future_nonce: with each
    wallet's valid original in the same request (mempool rule v1, rule 3),
    so each copy meets the original still pending; the originals must
    commit. The release CLI signs deterministically, so its re-signed copy
    is the original's bytes (the engine's cache), and the row says so.
An attempt counts only when the node refused it for the type's reason. The
node admitting one is a success (a defect): its wallet is read again and
the campaign goes on. A refusal for another reason is counted apart and
fails the type (the attempt did not test what it names); a transport error
is counted apart and the attempt is made again, and a wallet whose nonce
moved without a commit of ours marks the attempts since as admitted. After
each type a positive control (a valid send) must be admitted and commit, so
a channel that refuses everything cannot pass.

Not built yet: fee_cap_below_minimum and too_many_actions. The release CLI
checks a fee cap and the action count against the profile before it signs,
and the profile is bound by its digest, so they need a signer that skips
the CLI's own checks. Until then they count zero attempts, and the family's
attempts (the fewest over every type) cannot reach the target.

verify recomputes every count, status and digest from the rows. The rows
and the evidence name no address; the evidence binds the genesis files.
"""
import argparse
import base64
from collections import Counter
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import adversary  # noqa: E402
import config_checks  # noqa: E402
import load  # noqa: E402

SCHEMA = 'dytallix.attack-campaign.v1'
FAMILY = 'transactions'
PASS, FAIL = 'PASS', 'FAIL'
TARGET = 3000
SIGNATURE = adversary.MLDSA65_SIGNATURE_BYTES
STALE = 'Ordinary spending nonce is stale'
# Each approved transaction attack type (launch-rehearsals.md, Statistical
# campaign) and the reason the node must refuse it for; None: not built yet.
# The registry is the code's, never the record's: verify recomputes it.
REASONS = {
    'invalid_signature': 'invalid ordinary signature',
    'wrong_key_signature': 'invalid ordinary signature',
    'truncated_signature': 'Malformed ordinary envelope',
    'malformed_envelope': 'Malformed ordinary envelope',
    'non_canonical_envelope': 'Malformed ordinary envelope',
    'oversized_envelope': 'too large',
    'wrong_chain': 'Ordinary signed domain differs from the chain domain',
    'wrong_network': 'Malformed ordinary envelope',
    'expired': 'Ordinary submission expired or exceeds configured lifetime',
    'stale_nonce': STALE,
    'future_nonce': STALE,
    'conflicting_nonce': 'Counter is already reserved; explicit eviction is required',
    'duplicate': adversary.CACHE_REFUSAL,
    'resigned_copy': adversary.CACHE_REFUSAL,  # or the identity check, for byte-distinct copies
    'overspend': 'Insufficient current eligible funding for fee cap and actions',
    'fee_cap_below_minimum': None,
    'too_many_actions': None,
}
# Refused before admission (the RPC size limit, the engine's cache): a
# JSON-RPC error with the reason, not a CheckTx code.
BEFORE_ADMISSION = {'oversized_envelope', 'duplicate', 'resigned_copy'}
PENDING = ('duplicate', 'resigned_copy', 'conflicting_nonce', 'future_nonce')
MUTATED = ('invalid_signature', 'truncated_signature', 'malformed_envelope', 'non_canonical_envelope',
           'oversized_envelope')
SIGNED = ('wrong_chain', 'wrong_network', 'wrong_key_signature', 'stale_nonce', 'overspend')
OUTCOMES = ('refused', 'refused_other', 'admitted', 'transport')
OVERSPEND_UDRT = 10**17
OTHER_CHAIN = 'dytallix-attack-other-1'


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def built(kind):
    if kind not in REASONS:
        raise Failed(f'{kind!r} is not an approved transaction attack type')
    return REASONS[kind] is not None


def reason(kind, same_bytes=None):
    if kind == 'resigned_copy' and same_bytes is False:
        return adversary.IDENTITY_REFUSAL
    return REASONS[kind]


def classify(kind, code, log, same_bytes=None):
    """An attempt's outcome from the node's answer alone."""
    if not built(kind):
        raise Failed(f'{kind} is not built')
    if kind == 'resigned_copy' and not isinstance(same_bytes, bool):
        raise Failed('a re-signed copy must say whether its bytes were the original\'s')
    if not isinstance(code, int):
        return 'transport'
    if code == 0:
        return 'admitted'
    if code < 0 and kind not in BEFORE_ADMISSION:
        return 'refused_other'
    return 'refused' if reason(kind, same_bytes) in (log or '') else 'refused_other'


# Mutations of one valid transport ({"type":"ordinary_v2","envelope_base64":...}),
# a different byte or length for each attempt k.

def _wire(tx):
    wire = json.loads(tx)
    return wire, base64.b64decode(wire['envelope_base64'])


def _rewrap(wire, envelope):
    return json.dumps(dict(wire, envelope_base64=base64.b64encode(bytes(envelope)).decode())).encode()


def mutate(kind, tx, k):
    wire, envelope = _wire(tx)
    if len(envelope) <= SIGNATURE:
        raise Failed('the envelope is smaller than one signature')
    if kind == 'invalid_signature':
        body = bytearray(envelope)
        body[len(body) - 1 - k % (SIGNATURE - 1)] ^= 1 + k // (SIGNATURE - 1) % 255
        return _rewrap(wire, body)
    if kind == 'truncated_signature':
        return _rewrap(wire, envelope[:-(1 + k % (SIGNATURE - 1))])
    if kind == 'non_canonical_envelope':
        return _rewrap(wire, envelope + k.to_bytes(4, 'big') + b'\x00')
    if kind == 'malformed_envelope':
        return json.dumps(dict(wire, envelope_base64=f'!!not base64 {k}!!')).encode()
    if kind == 'oversized_envelope':
        return adversary.oversized_envelope(300_000 + k)
    raise Failed(f'{kind} is not a mutation')


# The run

class Campaign:
    """The attempts through one wallet set; `sign(wallet, nonce, amount,
    variant, expiry=None)` gives (transaction ID, transport) and `send(rows)`
    broadcasts [(row, tx)] in one request (load.broadcast)."""

    def __init__(self, run, rpc, sign, attempts, show=load.say, sleep=time.sleep, clock=time.monotonic,
                 block_time=5.0, recipient=None):
        self.run, self.rpc, self.sign, self.n = run, rpc, sign, attempts
        self.show, self.sleep, self.clock, self.block_time = show, sleep, clock, block_time
        self.rows = []
        self.loads = run.role('load')
        self.recipient = recipient or self.loads[-1].account_id
        self.expected = {w.label: None for w in self.loads}

    # The node

    def head(self):
        return load.head_height(self.rpc.one('head', 'abci_info', {}))

    def nonces(self, wallets):
        views = load.queries(self.rpc, 'accounts', [f'/ordinary/account/{w.account_id}' for w in wallets])
        for wallet, view in zip(wallets, views):
            wallet.view, wallet.nonce = view, int(view['spending_nonce'])
        return {w.label: w.nonce for w in wallets}

    def committed(self, ids, timeout):
        """Waits for each transaction's receipt; the IDs that never came."""
        missing, deadline = set(ids), self.clock() + timeout
        while missing and self.clock() < deadline:
            wanted = sorted(missing)
            found = load.queries(self.rpc, 'receipts', [f'/ordinary/receipt/{i}' for i in wanted])
            missing -= {i for i, receipt in zip(wanted, found) if receipt is not None}
            if missing:
                self.sleep(min(1.0, self.block_time / 5))
        return missing

    # Rows

    def record(self, kind, k, wallet, row, same_bytes=None):
        outcome = classify(kind, row.get('code'), row.get('log'), same_bytes)
        found = {'type': kind, 'k': k, 'wallet': wallet.label, 'code': row.get('code'),
                 'log': (row.get('log') or '')[:160], 'outcome': outcome}
        if kind == 'resigned_copy':
            found['same_bytes'] = same_bytes
        self.rows.append(found)
        return outcome

    def send(self, batch):
        """[(kind, k, wallet, tx, same_bytes)] in one request; the outcomes."""
        rows = [(load.new_row('attack', kind, wallet), tx) for kind, _, wallet, tx, _ in batch]
        try:
            load.broadcast(self.rpc, rows, 'attack')
        except load.Transport:
            pass  # each row stays without a code: a transport outcome
        return [self.record(kind, k, wallet, row, same) for (kind, k, wallet, _, same), (row, _) in zip(batch, rows)]

    def guard(self, wallets):
        """A wallet whose nonce moved past what our own commits explain had
        an attempt admitted: the attempts since its last check count so."""
        now = self.nonces(wallets)
        for wallet in wallets:
            want = self.expected.get(wallet.label)
            if want is not None and now[wallet.label] > want:
                since = [r for r in self.rows if r['wallet'] == wallet.label and r.get('checked') is None
                         and r['outcome'] in ('refused', 'refused_other', 'transport')]
                if since:
                    since[-1]['outcome'], since[-1]['moved_nonce'] = 'admitted', True
            self.expected[wallet.label] = now[wallet.label]
            for r in self.rows:
                if r['wallet'] == wallet.label:
                    r['checked'] = True

    def control(self, wallet):
        """A valid send that must be admitted and commit."""
        self.nonces([wallet])
        transaction_id, tx = self.sign(wallet, wallet.nonce, 1 + len(self.rows), 'valid')
        row = load.new_row('attack', 'control', wallet)
        try:
            load.broadcast(self.rpc, [(row, tx)], 'control')
        except load.Transport:
            pass
        missing = self.committed([transaction_id], self.block_time * 30) if row.get('code') == 0 else {transaction_id}
        self.rows.append({'type': 'control', 'wallet': wallet.label, 'code': row.get('code'),
                          'log': (row.get('log') or '')[:160], 'committed': not missing})
        self.expected[wallet.label] = wallet.nonce + 1
        return not missing

    # Attack types

    def counted(self, kind):
        return sum(1 for r in self.rows if r['type'] == kind and r['outcome'] == 'refused')

    def tries_left(self, kind):
        made = sum(1 for r in self.rows if r['type'] == kind)
        return made < 3 * self.n

    def single(self, kind):
        """The mutated and the falsely signed types, one wallet."""
        wallet = self.loads[0]
        self.nonces([wallet])
        self.expected[wallet.label] = wallet.nonce
        base = None
        k = 0
        while self.counted(kind) < self.n and self.tries_left(kind):
            batch = []
            while len(batch) < load.MAX_BATCH - 1 and self.counted(kind) + len(batch) < self.n:
                if kind in MUTATED:
                    if base is None:
                        base = self.sign(wallet, wallet.nonce, 1, 'valid')[1]
                    tx = mutate(kind, base, k)
                elif kind == 'stale_nonce':
                    tx = self.sign(wallet, wallet.nonce - 1, 1 + k, 'valid')[1]
                elif kind == 'overspend':
                    tx = self.sign(wallet, wallet.nonce, OVERSPEND_UDRT + k, 'valid')[1]
                else:
                    tx = self.sign(wallet, wallet.nonce, 1 + k, kind)[1]
                batch.append((kind, k, wallet, tx, None))
                k += 1
            outcomes = self.send(batch)
            if 'admitted' in outcomes or 'transport' in outcomes:
                self.guard([wallet])
                base = None
        self.guard([wallet])

    def expired(self):
        """Valid transactions with an expiry two blocks ahead, held past it."""
        wallet = self.loads[0]
        k = 0
        while self.counted('expired') < self.n and self.tries_left('expired'):
            self.nonces([wallet])
            self.expected[wallet.label] = wallet.nonce
            want = min(self.n - self.counted('expired'), 200)
            held = []
            for _ in range(want):
                held.append(('expired', k, wallet, self.sign(wallet, wallet.nonce, 1 + k, 'valid',
                                                             expiry='soon')[1], None))
                k += 1
            expiry = max(self.expiry_of(h[3]) for h in held)
            while self.head() <= expiry:
                self.sleep(self.block_time / 2)
            for n in range(0, len(held), load.MAX_BATCH - 1):
                self.send(held[n:n + load.MAX_BATCH - 1])
            self.guard([wallet])

    def expiry_of(self, tx):
        return self.sign.expiries.pop(digest(tx))

    def pending(self):
        """Each wallet's original with its four copies in one request; the
        originals must commit."""
        k = 0
        while (min(self.counted(kind) for kind in PENDING) < self.n
               and all(self.tries_left(kind) for kind in PENDING)):
            self.nonces(self.loads)
            originals = {}
            for wallet in self.loads:
                if min(self.counted(kind) for kind in PENDING) >= self.n:
                    break
                nonce, amount = wallet.nonce, 1 + k
                original_id, original = self.sign(wallet, nonce, amount, 'valid')
                resigned = self.sign(wallet, nonce, amount, 'valid')[1]
                batch = [('duplicate', k, wallet, original, None),
                         ('resigned_copy', k, wallet, resigned, resigned == original),
                         ('conflicting_nonce', k, wallet, self.sign(wallet, nonce, amount + 1, 'valid')[1], None),
                         ('future_nonce', k, wallet, self.sign(wallet, nonce + 1, amount, 'valid')[1], None)]
                row = load.new_row('attack', 'pending_original', wallet)
                rows = [(load.new_row('attack', kind, wallet), tx) for kind, _, _, tx, _ in batch]
                try:
                    load.broadcast(self.rpc, [(row, original)] + rows, 'pending copies')
                except load.Transport:
                    pass
                for (kind, kk, w, _, same), (r, _) in zip(batch, rows):
                    self.record(kind, kk, w, r, same)
                originals[wallet.label] = (original_id, row.get('code'))
                k += 1
            ids = [i for i, code in originals.values() if code == 0]
            missing = self.committed(ids, self.block_time * 30)
            for label, (transaction_id, code) in originals.items():
                self.rows.append({'type': 'pending_original', 'wallet': label, 'code': code,
                                  'committed': code == 0 and transaction_id not in missing})
                self.expected[label] = self.run.wallets[label].nonce + (1 if code == 0 else 0)
            self.guard(self.loads)

    def make(self, kinds):
        for kind in kinds:
            if not built(kind):
                self.show(f'== {kind}: not built yet (the release CLI will not sign it)')
                continue
            if kind in PENDING:
                # One round makes all four; it runs once, at the first of them.
                if not any(r['type'] in PENDING for r in self.rows):
                    self.show(f'== the pending copies: an original and its four copies, {self.n} of each')
                    self.pending()
            elif kind == 'expired':
                self.show(f'== expired: {self.n} attempts held past their expiry')
                self.expired()
            else:
                self.show(f'== {kind}: {self.n} attempts')
                self.single(kind)
            counts = Counter(r['outcome'] for r in self.rows if r['type'] == kind)
            first = next((r['log'] for r in self.rows if r['type'] == kind and r['outcome'] != 'admitted'), '')
            self.show(f'== {kind}: {dict(counts)}; the node said {first[:120]!r}')
            if not self.control(self.loads[0]):
                self.show(f'== the positive control after {kind} did not commit')
        return self.rows


# The record

def tally(rows, attempts):
    """Each type's outcome counts, the first log and its status, from rows:
    PASS once the wanted attempts were refused for the type's reason with no
    success and no other refusal."""
    types = {}
    for kind in REASONS:
        mine = [r for r in rows if r['type'] == kind]
        counts = {o: sum(1 for r in mine if r['outcome'] == o) for o in OUTCOMES}
        if not built(kind):
            status = 'not_built'
        elif counts['admitted'] or counts['refused_other']:
            status = FAIL
        elif not mine:
            status = 'not_run'
        else:
            status = PASS if counts['refused'] >= attempts else FAIL
        types[kind] = {'reason': REASONS[kind], 'counts': counts, 'status': status,
                       'first_log': next((r['log'] for r in mine), None)}
    return types


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, started, finished):
    for row in rows:
        if row['type'] in REASONS and row['type'] not in ('control', 'pending_original'):
            if classify(row['type'], row['code'], row['log'], row.get('same_bytes')) != row['outcome'] \
                    and not row.get('moved_nonce'):
                raise Failed(f'a {row["type"]} row\'s outcome does not follow from its code and log')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    originals = [r for r in rows if r['type'] == 'pending_original']
    checks = {
        'types': all(t['status'] in (PASS, 'not_built', 'not_run') for t in types.values()),
        'controls': bool(controls) and all(r['committed'] for r in controls),
        'originals': all(r['committed'] for r in originals),
    }
    body = {'schema': SCHEMA, 'family': FAMILY, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'started': started, 'finished': finished, 'attempts_wanted': attempts, 'types': types,
            'controls': {'sent': len(controls), 'committed': sum(r['committed'] for r in controls)},
            'originals': {'sent': len(originals), 'committed': sum(r['committed'] for r in originals)},
            'successes': sum(t['counts']['admitted'] for t in types.values()),
            'rows_sha256': digest(raw), 'checks': checks,
            'status': PASS if all(checks.values()) else FAIL}
    body['sha256'] = digest(canonical(body))
    return body


def rows_bytes(rows):
    return b''.join(canonical({k: v for k, v in r.items() if k != 'checked'}) + b'\n' for r in rows)


def verify(record, raw, genesis_bytes, engine_genesis_bytes):
    """Re-evaluates a campaign record from its rows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
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
                         record['started'], record['finished'])
    if again != record:
        raise Failed('the counts and statuses do not follow from the rows')
    return record['status']


# The release CLI's signing, with the falsified views an attacker can give it

class Signing:
    """sign(wallet, nonce, amount, variant, expiry=None) -> (ID, transport)."""

    def __init__(self, run, chain_id, expiry_blocks):
        self.run, self.chain_id, self.expiry_blocks = run, chain_id, expiry_blocks
        self.expiries = {}
        self.files = {}

    def profile(self, variant):
        path, profile = self.run.views
        if variant != 'wrong_chain':
            return path, profile
        false = copy.deepcopy(profile)
        false['context']['chain_id'] = OTHER_CHAIN
        if false['context']['height'] not in self.files:
            self.files.clear()
            target = Path(path).with_name(f'wrong-chain-{false["context"]["height"]}.json')
            target.write_text(json.dumps(false))
            self.files[false['context']['height']] = target
        return self.files[false['context']['height']], false

    def __call__(self, wallet, nonce, amount, variant, expiry=None):
        self.run.refresh_profile() if self.run.profile_stale() else None
        path, profile = self.profile(variant)
        view, key_file = copy.deepcopy(wallet.view), wallet.key_file
        if variant == 'wrong_chain':
            view['domain']['chain_id'] = OTHER_CHAIN
        elif variant == 'wrong_network':
            public = bytes(view['current_key']['public_key'])
            account_id, address = config_checks.origin_account(1, self.chain_id, public)
            view['domain']['network'] = 1
            view['domain']['account_id'] = view['account_id'] = account_id.hex()
            view['address'] = address
        elif variant == 'wrong_key_signature':
            other = next(w for w in self.run.role('load') if w is not wallet)
            view['current_key'] = copy.deepcopy(other.view['current_key'])
            key_file = other.key_file
        height = int(profile['context']['height'])
        held = expiry == 'soon'
        expiry = height + 2 if held else height + 1 + self.expiry_blocks
        recipient = self.run.funder.account_id if self.run.funder else self.run.role('load')[-1].account_id
        actions = [load.send_action(recipient, 'udrt', amount)]
        transaction_id, tx = self.run.signer.sign(path, profile, view, nonce, actions, expiry, load.GAS_LIMIT,
                                                  load.GAS_LIMIT * self.run.gas_price, key_file=key_file)
        if held:
            self.expiries[digest(tx)] = expiry
        return transaction_id, tx


def run_campaign(cli, home, fleet_dir, out, application_config, funder_label, validator, attempts, kinds,
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
        signer = load.Signer(cli, env, out / 'scratch')
        run = load.Load(rpc, signer, env, cli, fleet, out, settings, funder, validator, config, show=show)
        if len(run.role('load')) < 2:
            raise Failed('the campaign needs at least two load wallets (the wrong-key attempts sign with another)')
        run.fund()
        run.first_spends()
        run.refresh_profile()
        campaign = Campaign(run, rpc, Signing(run, fleet['chain_id'], settings.expiry_blocks), attempts, show=show,
                            block_time=settings.block_time)
        campaign.nonces(run.role('load'))
        rows = campaign.make(kinds)
        raw = rows_bytes(rows)
        (out / 'attacks.jsonl').write_bytes(raw)
        record = build_record(fleet['chain_id'], native_genesis, engine_genesis, rows, attempts, started,
                              load.now_utc())
        # The rows beside the record, as results.py reads them (FILE.json, FILE.jsonl).
        (out / 'attacks.json').write_text(json.dumps(record, indent=2) + '\n')
        return record
    finally:
        gateway.stop()


def report(record, show=load.say):
    for kind, t in record['types'].items():
        show(f'== {kind}: {t["status"]} {t["counts"]}; first: {(t["first_log"] or "")[:100]!r}')
    show(f'== controls {record["controls"]}, originals {record["originals"]}, successes {record["successes"]}: '
         f'{record["status"]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    r = commands.add_parser('run')
    for name in ('--cli', '--home', '--fleet', '--out', '--application-config', '--genesis', '--engine-genesis'):
        r.add_argument(name, type=Path, required=True)
    r.add_argument('--funder', required=True)
    r.add_argument('--validator', required=True)
    r.add_argument('--attempts', type=int, default=TARGET)
    r.add_argument('--types', default=','.join(REASONS))
    r.add_argument('--passphrase', type=Path)
    v = commands.add_parser('verify')
    for name in ('--evidence', '--rows', '--genesis', '--engine-genesis'):
        v.add_argument(name, type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'verify':
            status = verify(json.loads(args.evidence.read_bytes()), args.rows.read_bytes(),
                            args.genesis.read_bytes(), args.engine_genesis.read_bytes())
            print(status)
            return 0 if status == PASS else 1
        kinds = [k for k in args.types.split(',') if k]
        for kind in kinds:
            built(kind)
        record = run_campaign(args.cli, args.home, args.fleet, args.out, args.application_config, args.funder,
                              args.validator, args.attempts, kinds, args.genesis.read_bytes(),
                              args.engine_genesis.read_bytes(), passphrase=args.passphrase)
        report(record)
        return 0 if record['status'] == PASS else 1
    except (Failed, load.Failed, adversary.Failed, OSError, KeyError, ValueError) as error:
        print(f'attacks: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
