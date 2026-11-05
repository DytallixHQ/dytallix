#!/usr/bin/env python3
"""The attack campaign's staking family (simulation plan, statistical
campaign; launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json, its
types as P01_T04_ATTACK_TYPES_2026-11-04.json settles them): each type at
least 3,000 times on the staging hosts, with zero successes.

  staking.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                    --engine-genesis ENGINE_GENESIS

`staging_network.py staking-campaign` runs it on the staging network: a
fresh fleet funded from the ecosystem growth account, and the staging
genesis's campaign operator, approved for a validator ID the genesis does
not register. Every attempt is signed by the release CLI (the possession
proofs by the release's validator key tool) and goes through the client
channel. Each type and what it sends:
  - bond_below_minimum: the campaign validator's registration by its
    operator, with a fresh consensus key and its valid proof, its self-bond
    1 + k udgt below the minimum;
  - registration_reused_key: the same with a full self-bond, its key and
    proof those of a registered validator (on the staging network the
    validator host's own key, read where it is installed);
  - registration_bad_proof: the same with the fresh key and a full
    self-bond, one byte of its valid proof changed (byte k);
  - bond_to_unknown_validator: a bond of 1 + k udgt to a validator ID no
    validator has;
  - unbond_more_than_bonded: unbonding 1 + k udgt more than the wallet has
    bonded to a validator;
  - early_withdrawal: withdrawing the wallet's own unbonding entry long
    before it matures;
  - repeated_reward_claim: a reward claim by a wallet whose whole bond is
    unbonded and whose rewards were claimed, so it accrues nothing.
The bad proof is refused at admission (CheckTx) and runs first, one attempt
at a time. The others pass admission and fail when the block runs, charged
their fee, so they go in rounds: one attempt in flight per wallet, each
followed to its receipt, its wallet's account read before and after. Such
an attempt counts as refused only when its receipt names the type's rule
and the reads show nothing changed but the fee the receipt charged and the
one nonce it spent: the wallet's liquid DGT, bonds, pending bond and
unbonding hold, and for the registrations and bonds so does the validator
set. A repeated claim commits and pays nothing; it counts as refused when
the reads show nothing changed but that fee and nonce, its claimable
rewards 0 before and after (P01_T04_ATTACK_TYPES_2026-11-04). Each row
keeps its attempt's fault as read from the chain (the key against the
validator set, the proof, the amount against the minimum or the bond, the
target, the entry's age) and counts only when the fault is the type's.

An attempt the chain accepted, or one that changed what it must not, is a
success (a defect), and its type stops: an accepted registration stops all
three. So is a lost attempt whose wallet's nonce then moved: what it did
was never read. A refusal for another reason is counted apart and fails the type (the
attempt did not test what it names). An attempt the channel could not make,
or one admitted that never committed, is counted apart and made again. The
positive controls: before and after the attempts a valid registration of the
campaign validator is checked at admission and would be admitted (it is
never sent); the repeated claimer's first claim paid; after the attempts a
valid bond and a valid unbond commit.

verify recomputes every outcome, count, status and digest from the rows,
which name no address; the evidence binds the genesis files.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import attacks  # noqa: E402
import load  # noqa: E402

SCHEMA = attacks.SCHEMA
FAMILY = 'staking'
PASS, FAIL = attacks.PASS, attacks.FAIL
TARGET = 3000
OUTCOMES = attacks.OUTCOMES
# The node's rule for each type (ordinary_validator.rs). A repeated claim
# has none: it commits and pays nothing.
REASONS = {
    'bond_below_minimum': 'VALIDATOR_SELF_BOND_MINIMUM',
    'registration_reused_key': 'CONSENSUS_KEY_ALREADY_USED',
    'registration_bad_proof': 'INVALID_VALIDATOR_PROOF',
    'bond_to_unknown_validator': 'BOND_TARGET_UNAVAILABLE',
    'unbond_more_than_bonded': 'UNBOND_PRINCIPAL_UNAVAILABLE',
    'early_withdrawal': 'UNBOND_NOT_MATURE',
    'repeated_reward_claim': 'a claim that pays nothing',
}
REGISTRATIONS = ('bond_below_minimum', 'registration_reused_key', 'registration_bad_proof')
PAYS_NOTHING = 'repeated_reward_claim'
# Refused at admission: they spend no nonce, so they run first, one at a time.
AT_ADMISSION = ('registration_bad_proof',)
# What a refused attempt's wallet must keep. Claimable rewards are left out:
# a bonded wallet's rise every block (the claimer's are checked apart).
HELD = ('liquid_udgt', 'bonded_udgt', 'bonds', 'pending_bond_udgt', 'unbonding_udgt', 'unbonds')
# The types whose rows read the validator set as well.
SET_HELD = REGISTRATIONS + ('bond_to_unknown_validator',)
PROOF_BYTES = 3309
TRIES = 3
SUCCESS, FAILURE = 'success', 'application_failure'


class Failed(Exception):
    pass


class Unavailable(Exception):
    """The channel could not take the attempts (no answer from the node)."""


digest, rows_bytes = attacks.digest, attacks.rows_bytes


def record_digest(value):
    return digest(attacks.canonical(value))


# Outcomes

def as_named(row):
    """Whether an attempt carried its type's fault, as read from the chain,
    and no other."""
    kind, fault = row['type'], row.get('fault') or {}
    amount = int(fault.get('amount_udgt') or 0)
    if kind in REGISTRATIONS:
        minimum = int(fault['minimum_udgt'])
        key, proof = fault.get('key'), fault.get('proof')
        if kind == 'bond_below_minimum':
            return key == 'fresh' and proof == 'valid' and 0 < amount < minimum
        if kind == 'registration_reused_key':
            return key == 'registered' and proof == 'valid' and amount >= minimum
        return key == 'fresh' and proof == 'altered' and amount >= minimum
    if kind == 'bond_to_unknown_validator':
        return fault.get('target_known') is False and amount > 0
    if kind == 'unbond_more_than_bonded':
        return fault.get('target_known') is True and amount > int(fault['bonded_udgt']) > 0
    if kind == 'early_withdrawal':
        return fault.get('owned') is True and fault['height'] < fault['matures_at']
    if kind == PAYS_NOTHING:
        return fault.get('bonded_udgt') == '0' and fault.get('claimed_before') is True
    return False


def only_fee(row):
    """Nothing changed in the wallet (or the validator set) but the fee its
    receipt charged and the one nonce it spent."""
    state, receipt = row['state'], row['receipt']
    return (not state['changed'] and state['nonce_step'] == 1
            and receipt['nonce_after'] == receipt['nonce_before'] + 1
            and int(state['drt_change']) == -int(receipt['charge'])
            and not state.get('validators_changed'))


def classify(row):
    """An attempt's outcome from the node's answers and the reads."""
    kind = row['type']
    if kind not in REASONS:
        raise Failed(f'{kind!r} is not an approved staking attack type')
    code = row.get('code')
    if not isinstance(code, int) or isinstance(code, bool):
        return 'transport'
    if not as_named(row):
        return 'refused_other'
    if code != 0:
        return 'refused' if REASONS[kind] in (row.get('log') or '') else 'refused_other'
    receipt = row.get('receipt')
    if receipt is None or row.get('state') is None:
        return 'transport'  # admitted, and expired without a commit
    if receipt['outcome'] == SUCCESS:
        if kind != PAYS_NOTHING:
            return 'admitted'
        return 'refused' if only_fee(row) and row['state']['claimable'] == ['0', '0'] else 'admitted'
    if kind == PAYS_NOTHING or receipt['outcome'] != FAILURE or receipt.get('rule_code') != REASONS[kind]:
        return 'refused_other'
    return 'refused' if only_fee(row) else 'admitted'


def said(row):
    """What the node said: its admission log, else the receipt's rule."""
    if row.get('code') not in (None, 0):
        return row.get('log') or ''
    receipt = row.get('receipt')
    if receipt:
        return receipt.get('rule_code') or receipt['outcome']
    return row.get('error') or row.get('log')


def state_facts(before, after, set_before=None, set_after=None):
    """What a row keeps of the reads of its wallet before and after it."""
    facts = {'changed': sorted(f for f in HELD if before[f] != after[f]),
             'nonce_step': int(after['nonce']) - int(before['nonce']),
             'drt_change': str(int(after['liquid_udrt']) - int(before['liquid_udrt'])),
             'claimable': [str(before['claimable_rewards_udrt']), str(after['claimable_rewards_udrt'])]}
    if set_before is not None:
        facts['validators_changed'] = set_before != set_after
    return facts


def receipt_facts(receipt):
    return {'outcome': receipt['outcome'], 'rule_code': receipt.get('rule_code'), 'charge': str(receipt['charge']),
            'nonce_before': int(receipt['nonce_before']), 'nonce_after': int(receipt['nonce_after']),
            'block_height': int(receipt['block_height'])}


def control_ok(row):
    if row['what'] == 'registration':
        return row.get('code') == 0
    receipt = row.get('receipt') or {}
    if row['what'] == 'first_claim':
        return receipt.get('outcome') == SUCCESS and int(row.get('paid_udrt') or 0) > 0
    return row.get('code') == 0 and receipt.get('outcome') == SUCCESS


# The run

class Campaign:
    """Each type's attempts against the node, through the desk."""

    def __init__(self, desk, attempts, show=print):
        self.desk, self.n, self.show = desk, attempts, show
        self.rows, self.last = [], {}

    def mine(self, kind):
        return [r for r in self.rows if r['type'] == kind]

    def made(self, kind):
        return sum(1 for r in self.mine(kind) if r['outcome'] != 'transport')

    def finished(self, kind):
        mine = self.mine(kind)
        if any(r['outcome'] == 'admitted' for r in mine):
            return True
        if kind in REGISTRATIONS and any(r['outcome'] == 'admitted' for k in REGISTRATIONS for r in self.mine(k)):
            return True
        running = 0
        for row in reversed(mine):
            if row['outcome'] != 'transport':
                break
            running += 1
        if running >= TRIES:
            raise Failed(f'{kind} attempt {self.made(kind)} could not be made {TRIES} times running')
        return self.made(kind) >= self.n

    def guard(self, reads):
        """A wallet whose nonce moved since its last read had an attempt
        the channel lost committed: its last such attempt counts as
        admitted, since what it did was never read."""
        for wallet, read in reads.items():
            last = self.last.get(wallet)
            if last is None or int(read['nonce']) == last:
                continue
            lost = [r for r in self.rows if r.get('wallet') == wallet and r['outcome'] == 'transport']
            if int(read['nonce']) < last or not lost:
                raise Failed(f'{wallet}\'s nonce moved from {last} to {read["nonce"]} with no attempt to explain it')
            lost[-1].update(outcome='admitted', moved_nonce=True)

    def round(self, batch):
        """[(kind, k)], each on its own wallet: one request, each admitted
        attempt followed to its receipt, the wallets read before and after."""
        desk = self.desk
        wallets = [desk.wallet(kind) for kind, _ in batch]
        if len(set(wallets)) != len(wallets):
            raise Failed('a round has two attempts on one wallet')
        before, validators = desk.accounts(wallets), desk.validators()
        self.guard(before)
        forged = [desk.forge(kind, k, before[w], validators) for (kind, k), w in zip(batch, wallets)]
        try:
            answers = desk.send([tx for _, tx, _ in forged])
        except Unavailable as error:
            answers = [{'error': str(error)[:200]}] * len(batch)
        admitted = [ident for (ident, _, _), answer in zip(forged, answers) if answer.get('code') == 0]
        receipts = desk.wait(admitted) if admitted else {}
        after = desk.accounts(wallets) if admitted else before
        validators_after = desk.validators() if admitted else validators
        self.last.update({w: int(after[w]['nonce']) for w in wallets})
        for (kind, k), wallet, (ident, _, fault), answer in zip(batch, wallets, forged, answers):
            row = {'type': kind, 'k': k, 'wallet': wallet, 'fault': fault}
            if 'code' in answer:
                row.update(code=answer['code'], log=(answer.get('log') or '')[:200])
            else:
                row['error'] = answer['error']
            if answer.get('code') == 0 and receipts.get(ident) is not None:
                row['receipt'] = receipts[ident]
                held = kind in SET_HELD
                row['state'] = state_facts(before[wallet], after[wallet], validators if held else None,
                                           validators_after if held else None)
            row['outcome'] = classify(row)
            self.rows.append(row)

    def control(self, what, when=None):
        row = {'type': 'control', 'what': what, **({'when': when} if when else {}), **self.desk.control(what)}
        self.rows.append(row)
        if not control_ok(row):
            self.show(f'== the positive control {what} {when or ""} failed: {said(row)!r}')

    def run(self, kinds):
        for kind in kinds:
            if kind not in REASONS:
                raise Failed(f'{kind!r} is not an approved staking attack type')
        self.rows.append({'type': 'control', 'what': 'first_claim', **self.desk.first_claim})
        self.control('registration', 'before')
        for kind in (k for k in kinds if k in AT_ADMISSION):
            self.show(f'== {kind}: {self.n} attempts, one at a time')
            while not self.finished(kind):
                self.round([(kind, self.made(kind))])
        rest = [k for k in kinds if k not in AT_ADMISSION]
        if rest:
            self.show(f'== {", ".join(rest)}: {self.n} attempts each, one a wallet a round')
        while True:
            # Each wallet's next attempt: of the kinds sharing it (the
            # registrations), the one with the fewest made.
            batch = {}
            for kind in rest:
                wallet = self.desk.wallet(kind)
                if not self.finished(kind) and (wallet not in batch or self.made(kind) < self.made(batch[wallet])):
                    batch[wallet] = kind
            if not batch:
                break
            self.round([(kind, self.made(kind)) for kind in batch.values()])
        for kind in kinds:
            counts = {o: sum(1 for r in self.mine(kind) if r['outcome'] == o) for o in OUTCOMES}
            first = next((said(r) for r in self.mine(kind)), None)
            self.show(f'== {kind}: {counts}; the node said {first!r}')
        self.control('registration', 'after')
        self.control('bond')
        self.control('unbond')
        return self.rows


def tally(rows, attempts):
    """Each type's outcome counts, first answer and status: PASS once the
    wanted attempts were refused with no success and no other refusal."""
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
                       'first_log': next((said(r) for r in mine), None)}
    return types


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, materials, started, finished):
    for row in rows:
        if row['type'] == 'control':
            continue
        # A lost attempt whose wallet's nonce moved counts as admitted.
        lost = row.get('moved_nonce') is True and classify(row) == 'transport' and row['outcome'] == 'admitted'
        if classify(row) != row['outcome'] and not lost:
            raise Failed(f'a {row["type"]} row\'s outcome does not follow from its answers and reads')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    wanted = {('first_claim', None), ('registration', 'before'), ('registration', 'after'), ('bond', None),
              ('unbond', None)}
    checks = {
        'types': all(t['status'] in (PASS, 'not_run') for t in types.values()),
        'controls': {(r['what'], r.get('when')) for r in controls} == wanted and all(map(control_ok, controls)),
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

BOND_DGT = 100           # each bonded wallet's bond to the validator
EARLY_UNBOND_DGT = 1     # the early withdrawer's unbonding entry
SETTLE_BLOCKS = 3        # blocks the claimer is watched to accrue nothing


def validator_set(view):
    """The set's identities, without powers (a bond changes those)."""
    return sorted([v['validator_id'], v['consensus_key_base64']] for v in view['validators'])


class Desk:
    """The campaign's wallets on the node. `wallets` maps the delegation
    types to the fleet's load wallets; the registrations use the operator."""

    def __init__(self, run, rpc, keytool, engine_genesis, operator, campaign_validator, validator, registered_key,
                 fresh_key, maturity_blocks, wallets, show=load.say, sleep=time.sleep, clock=time.monotonic):
        self.run, self.rpc, self.keytool, self.engine_genesis = run, rpc, str(keytool), str(engine_genesis)
        self.operator, self.campaign_validator, self.validator = operator, campaign_validator, validator
        self.keys = {'registered': str(registered_key), 'fresh': str(fresh_key)}
        self.maturity_blocks, self.show, self.sleep, self.clock = maturity_blocks, show, sleep, clock
        self.wallets = {w.label: w for w in [operator, *wallets.values()]}
        self.roles = {kind: operator.label for kind in REGISTRATIONS}
        self.roles.update({kind: w.label for kind, w in wallets.items()})
        self.expiries, self.unbond_id, self.first_claim, self.minimum = {}, None, None, None

    # Reads

    def head(self):
        return load.head_height(self.rpc.one('head', 'abci_info', {}))

    def wallet(self, kind):
        return self.roles[kind]

    def accounts(self, labels):
        """Each wallet's account summary, and its account view to sign from."""
        wallets = [self.wallets[label] for label in labels]
        self.run.sync(wallets)
        views = load.queries(self.rpc, 'accounts', [f'/account/{w.address}' for w in wallets])
        return dict(zip(labels, views))

    def validators(self):
        view = load.query(self.rpc, 'validators', '/staking/validators')
        self.minimum = int(view['min_self_bond_udgt'])
        return validator_set(view)

    def summary(self, wallet):
        return load.query(self.rpc, 'accounts', f'/account/{wallet.address}')

    # Attempts

    def proof(self, key, nonce, expiry, amount):
        """The release tool's ValidatorRegister action for the campaign
        validator, its proof signed by `key` for this nonce and amount."""
        result = subprocess.run([self.keytool, 'proof', '--key-file', self.keys[key], '--genesis', self.engine_genesis,
                                 '--operation', 'register', '--validator', self.campaign_validator,
                                 '--owner', self.operator.address, '--nonce', str(nonce), '--expiry-height',
                                 str(expiry), '--amount-udgt', str(amount)], capture_output=True, text=True, timeout=60)
        if result.returncode != 0:
            raise Failed(f'the validator key tool made no proof: {result.stderr.strip()[:300]}')
        return json.loads(result.stdout)['action']

    def registration(self, kind, k, validators):
        minimum = self.minimum
        amount = minimum - 1 - k if kind == 'bond_below_minimum' else minimum
        if amount <= 0:
            raise Failed('the attempts outnumber the minimum self-bond')
        key = 'registered' if kind == 'registration_reused_key' else 'fresh'
        expiry = int(self.run.profile['context']['height']) + 1 + self.run.s.expiry_blocks
        action = self.proof(key, self.operator.nonce, expiry, amount)
        public = base64.b64encode(bytes(action['consensus_key'])).decode()
        fault = {'key': 'registered' if public in {v[1] for v in validators} else 'fresh', 'proof': 'valid',
                 'amount_udgt': str(amount), 'minimum_udgt': str(minimum)}
        if fault['key'] != key:
            raise Failed(f'the {key} key is {fault["key"]} on the chain')
        if kind == 'registration_bad_proof':
            action['proof'][k % PROOF_BYTES] ^= 1 + k // PROOF_BYTES % 255
            fault['proof'] = 'altered'
        return action, fault

    def forge(self, kind, k, before, validators):
        """(transaction ID, transport, fault) of attempt k, signed at the
        wallet's committed nonce."""
        signed_at = int(self.run.profile['context']['height'])
        if signed_at + self.run.s.expiry_blocks // 2 <= int(before['context']['height']):
            self.run.refresh_profile()
        wallet = self.wallets[self.roles[kind]]
        gas = load.GAS_LIMIT
        known = {v[0] for v in validators}
        if kind in REGISTRATIONS:
            action, fault = self.registration(kind, k, validators)
            gas = load.FUNDING_GAS_LIMIT
        elif kind == 'bond_to_unknown_validator':
            target = f'unknown-validator-{k}'
            action = {'type': 'RewardBond', 'validator_id': target, 'amount_udgt': str(1 + k)}
            fault = {'target_known': target in known, 'amount_udgt': str(1 + k)}
        elif kind == 'unbond_more_than_bonded':
            bonded = next((int(b['amount_udgt']) for b in before['bonds'] if b['validator_id'] == self.validator), 0)
            amount = bonded + 1 + k
            action = {'type': 'RewardBeginUnbond', 'validator_id': self.validator, 'amount_udgt': str(amount)}
            fault = {'target_known': self.validator in known, 'bonded_udgt': str(bonded), 'amount_udgt': str(amount)}
        elif kind == 'early_withdrawal':
            entry = next((u for u in before['unbonds'] if u['unbond_id'] == self.unbond_id), None)
            action = {'type': 'ValidatorWithdraw', 'unbond_id': self.unbond_id}
            fault = {'owned': entry is not None, 'height': int(before['context']['height']),
                     'matures_at': int(entry['effective_height']) + self.maturity_blocks if entry else 0}
        elif kind == PAYS_NOTHING:
            action = {'type': 'RewardClaim'}
            fault = {'bonded_udgt': str(before['bonded_udgt']), 'claimed_before': self.first_claim is not None}
        else:
            raise Failed(f'{kind!r} is not an approved staking attack type')
        row, tx = self.run.sign(load.new_row('attack', kind, wallet), wallet, [action], gas)
        self.expiries[row['id']] = row['expiry_height']
        return row['id'], tx, fault

    def send(self, txs):
        """Each transport broadcast in one request: the node's answers."""
        try:
            results = self.rpc.call('attack', [('broadcast_tx_sync', {'tx': base64.b64encode(tx).decode()})
                                               for tx in txs])
        except load.Transport as error:
            raise Unavailable(str(error))
        answers = []
        for result in results:
            if isinstance(result, load.RpcError):
                answers.append({'code': -1, 'log': str(result)})
            else:
                answers.append({'code': int(result['code']), 'log': result.get('log') or ''})
        return answers

    def wait(self, ids):
        """Each transaction's receipt facts, or None once its expiry passed
        without one."""
        found, missing = {}, set(ids)
        while missing:
            wanted = sorted(missing)
            receipts = load.queries(self.rpc, 'receipts', [f'/ordinary/receipt/{i}' for i in wanted])
            for ident, receipt in zip(wanted, receipts):
                if receipt is not None:
                    if receipt['transaction_id'] != ident:
                        raise Failed(f'the receipt for {ident} is for another transaction')
                    found[ident] = receipt_facts(receipt)
                    missing.discard(ident)
            if missing:
                head = self.head()
                for ident in [i for i in missing if head > self.expiries[i]]:
                    found[ident] = None
                    missing.discard(ident)
                if missing:
                    self.sleep(self.run.s.poll_interval)
        return found

    def control(self, what):
        """A transaction the node should admit: a registration checked at
        admission and never sent, or a bond or unbond that must commit."""
        self.run.refresh_profile()
        validators = self.validators()
        if what == 'registration':
            wallet = self.operator
            self.accounts([wallet.label])
            action, _ = self.registration('registration_control', 0, validators)
            row, tx = self.run.sign(load.new_row('attack', 'control', wallet), wallet, [action], load.FUNDING_GAS_LIMIT)
            answer = self.rpc.one('check', 'check_tx', {'tx': base64.b64encode(tx).decode()})
            return {'code': int(answer.get('code') or 0), 'log': (answer.get('log') or '')[:200]}
        kind = {'bond': 'bond_to_unknown_validator', 'unbond': 'unbond_more_than_bonded'}[what]
        wallet = self.wallets[self.roles[kind]]
        self.accounts([wallet.label])
        action = {'type': 'RewardBond' if what == 'bond' else 'RewardBeginUnbond', 'validator_id': self.validator,
                  'amount_udgt': '1'}
        row, tx = self.run.sign(load.new_row('attack', 'control', wallet), wallet, [action])
        self.expiries[row['id']] = row['expiry_height']
        answer = self.send([tx])[0]
        receipt = self.wait([row['id']])[row['id']] if answer['code'] == 0 else None
        return {'code': answer['code'], 'log': answer['log'][:200], 'receipt': receipt}

    # Setup

    def online(self, wallet, *args):
        """One write through the CLI's online path, to its receipt."""
        kind = args[1] if args[0] == 'stake' else args[0]
        return self.run.follow(self.run.online(wallet, args, phase='setup', kind=kind))

    def until(self, what, ready, timeout=None):
        deadline = self.clock() + (timeout or self.run.s.block_time * 30)
        while not ready():
            if self.clock() > deadline:
                raise Failed(f'{what} did not happen in time')
            self.sleep(self.run.s.poll_interval)

    def prepare(self, attempts):
        """The operator's first spend, and DRT for its attempts' fee caps;
        the bonds; the early withdrawer's unbonding entry; the claimer
        unbonded whole and its rewards claimed, then watched to accrue
        nothing."""
        run, roles = self.run, {kind: self.wallets[label] for kind, label in self.roles.items()}
        run.prime([(self.operator, ('send', '--to', run.funder.address, '--amount', '1', '--token', 'drt'))])
        need = (2 * attempts + 10) * load.FUNDING_GAS_LIMIT * run.gas_price
        short = -(-(need - int(self.summary(self.operator)['liquid_udrt'])) // load.UNIT)
        if short > 0:
            self.show(f'== {run.funder.label} pays the operator {short} DRT for its attempts\' fee caps')
            self.online(run.funder, 'send', '--to', self.operator.address, '--amount', str(short), '--token', 'drt')
        bonded = [roles[k] for k in ('unbond_more_than_bonded', 'early_withdrawal', PAYS_NOTHING)]
        self.show(f'== {len(bonded)} wallets bond {BOND_DGT} DGT to {self.validator}')
        for wallet in bonded:
            self.online(wallet, 'stake', 'bond', '--validator', self.validator, '--amount', str(BOND_DGT))
        for wallet in bonded:
            self.until(f'{wallet.label}\'s bond', lambda w=wallet: int(self.summary(w)['bonded_udgt'])
                       == BOND_DGT * load.UNIT)
        early = roles['early_withdrawal']
        self.online(early, 'stake', 'unbond', '--validator', self.validator, '--amount', str(EARLY_UNBOND_DGT))
        entries = self.summary(early)['unbonds']
        if len(entries) != 1:
            raise Failed(f'{early.label} has {len(entries)} unbonding entries, not one')
        self.unbond_id = entries[0]['unbond_id']
        claimer = roles[PAYS_NOTHING]
        self.online(claimer, 'stake', 'unbond', '--validator', self.validator, '--amount', str(BOND_DGT))
        self.until(f'{claimer.label}\'s unbonding', lambda: int(self.summary(claimer)['bonded_udgt']) == 0)
        settled = self.head() + SETTLE_BLOCKS
        self.until('the claimer\'s last reward', lambda: self.head() > settled)
        before = self.summary(claimer)
        row = self.online(claimer, 'stake', 'claim')
        after = self.summary(claimer)
        paid = int(after['liquid_udrt']) - int(before['liquid_udrt']) + int(row['charge_udrt'])
        receipt = load.query(self.rpc, 'receipts', f'/ordinary/receipt/{row["id"]}')
        self.first_claim = {'receipt': receipt_facts(receipt), 'paid_udrt': str(paid)}
        self.show(f'== {claimer.label} claimed {paid} udrt; it must accrue nothing more')
        watched = self.head() + SETTLE_BLOCKS
        while self.head() <= watched:
            claimable = int(self.summary(claimer)['claimable_rewards_udrt'])
            if claimable:
                raise Failed(f'{claimer.label} still accrues rewards ({claimable} udrt) with nothing bonded')
            self.sleep(run.s.poll_interval)


def materials_of(desk):
    return {'campaign_validator': desk.campaign_validator, 'validator': desk.validator,
            'minimum_udgt': str(desk.minimum), 'bond_udgt': str(BOND_DGT * load.UNIT),
            'early_unbond_udgt': str(EARLY_UNBOND_DGT * load.UNIT), 'maturity_blocks': desk.maturity_blocks}


def generate_key(keytool, directory):
    """A fresh consensus key the chain has never seen."""
    directory = Path(directory).resolve()
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    key, state = directory / 'fresh-validator-key.json', directory / 'fresh-validator-state.json'
    result = subprocess.run([str(keytool), 'generate', '--key-file', str(key), '--state-file', str(state)],
                            capture_output=True, text=True, timeout=60)
    if result.returncode != 0:
        raise Failed(f'the validator key tool made no key: {result.stderr.strip()[:300]}')
    return key


def run_campaign(cli, keytool, home, fleet_dir, out, application_config, funder_label, operator_label,
                 campaign_validator, validator, registered_key, attempts, kinds, native_genesis, engine_genesis,
                 engine_genesis_path, show=load.say, passphrase=None, settings=None):
    out, fleet_dir = Path(out), Path(fleet_dir)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(home))
    if passphrase:
        env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    config = json.loads(Path(application_config).read_bytes())
    lifecycle = config['lifecycle']
    maturity = int(lifecycle['evidence_max_age_blocks']) + int(lifecycle['processing_margin_blocks'])
    fleet = json.loads((fleet_dir / 'fleet.json').read_bytes())
    fleet['_dir'] = str(fleet_dir)
    entries = {e['name']: e for e in json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())['entries']}
    funder = load.keystore_account(entries, funder_label, fleet['chain_id'])
    found = load.keystore_account(entries, operator_label, fleet['chain_id'])
    operator = load.Wallet(found['label'], 'operator', found['address'], found['account_id'],
                           keystore_name=found['label'])
    settings = settings or load.Settings()
    # Each attempt reserves a full fee cap, and fails charged.
    gas_price = int(config['ordinary']['fee_profile']['gas_price'])
    settings.fund_drt = max(settings.fund_drt, (attempts + 10) * load.GAS_LIMIT * gas_price)
    started = load.now_utc()
    gateway = load.Gateway(cli, env, out / 'gateway.log')
    show(f'== the gateway relays to {gateway.endpoint} at {gateway.url}')
    try:
        rpc = load.Rpc(gateway.url, settings.connections)
        run = load.Load(rpc, load.Signer(cli, env, out / 'scratch'), env, cli, fleet, out, settings, funder, validator,
                        config, show=show)
        loads = run.role('load')
        delegations = ('bond_to_unknown_validator', 'unbond_more_than_bonded', 'early_withdrawal', PAYS_NOTHING)
        if len(loads) < len(delegations):
            raise Failed(f'the campaign needs {len(delegations)} load wallets')
        run.wallets[operator.label] = operator
        fresh = generate_key(keytool, out / 'keys')
        desk = Desk(run, rpc, keytool, engine_genesis_path, operator, campaign_validator, validator, registered_key,
                    fresh, maturity, dict(zip(delegations, loads)), show=show)
        run.fund()
        run.first_spends()
        run.refresh_profile()
        desk.validators()
        desk.prepare(attempts)
        rows = Campaign(desk, attempts, show).run(kinds)
        record = build_record(fleet['chain_id'], native_genesis, engine_genesis, rows, attempts, materials_of(desk),
                              started, load.now_utc())
        # The rows beside the record, as results.py reads them (FILE.json, FILE.jsonl).
        (out / 'staking.jsonl').write_bytes(rows_bytes(rows))
        (out / 'staking.json').write_text(json.dumps(record, indent=2) + '\n')
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
        print(f'staking: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    sys.exit(main())
