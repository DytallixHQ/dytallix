#!/usr/bin/env python3
"""The attack campaign's governance family (simulation plan, statistical
campaign; launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json, its
types as P01_T04_ATTACK_TYPES_2026-11-04.json and
P01_T04_LATE_VOTES_2026-11-05.json settle them): each type at least 3,000
times on the staging hosts, with zero successes.

  governance.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                       --engine-genesis ENGINE_GENESIS

`staging_network.py governance-campaign` runs it on the staging network: a
fresh fleet funded from the ecosystem growth account, its proposer, depositor
and voters bonded to the validator, its load wallets never bonded. Every
attempt is a governance transaction the release CLI signs and sends through
the client channel in one step (`dytallix governance propose|deposit|vote`).
Each type and what it sends:
  - proposal_below_deposit: the proposer's no-op proposal (the active set
    size the chain has), then for odd k a deposit of 1 + k udgt from the
    proposer, for even k none: a proposal that collects less than the
    minimum deposit;
  - parameter_outside_bounds: a bonded voter's proposal of the active set
    size or the minimum self-bond just outside the genesis bounds (above or
    below, by k), at the chain's next proposal ID;
  - vote_without_bond: a vote by a load wallet, never bonded, so it has no
    weight in the open vote's snapshot;
  - repeated_vote: a second vote by a bonded voter whose first vote counted;
  - vote_after_period: a vote by a bonded voter that never voted, sent after
    the vote's end height.
The three votes need a vote open, which happens only when a proposal with
the minimum deposit reaches its deposit close. CI keeps the production
periods (120,960 blocks) and runs the first two types; the votes run where
the rehearsal timings open a vote after 720 blocks (the staging hosts), and
against a stand-in node in the tests.

A governance transaction has no receipt. Its result is read from the block
that holds it (block and block_results, the transaction found by the engine
hash the CLI reported): code 3 and the rule for a failure the block charged,
code 0 for a success. Each attempt's wallet is read before and after. The
types other than proposal_below_deposit count as refused only when the
result names the type's rule (for a late vote, VOTING_CLOSED or, once the
finished proposal is removed, PROPOSAL_UNKNOWN), and the reads show nothing
changed but the fee the block charged (the larger of the gas used and the
minimum gas, at the gas price) and the one nonce it spent: the wallet's
liquid DGT, bonds, pending bond and unbonding; for a vote, the voter's vote;
for a proposal, the chain's next proposal ID. A proposal below the deposit
commits by design (P01_T04_ATTACK_TYPES_2026-11-04): it counts as refused
when its proposal and deposit committed and the reads show it collecting
with exactly that deposit, below the minimum, and nothing changed in the
proposer's account but the two fees, the deposit and the refunds due. Each
row keeps its attempt's fault as read from the chain (the deposit against
the minimum, the value against the bounds, the proposer's bond, the ID
against the next one, the voter's snapshot weight and earlier vote, the
height against the vote's end) and counts only when the fault is the type's.

Where a deposit period ends within the run (the rehearsal timings), every
proposal the campaign made is read again at its close: removed, or finished
below the minimum, never voting; and the proposer's liquid DGT, read before
and after each attempt and at the end, must rise by each deposit at its
close. Every vote round's tally is read before and after: it may change only
when a vote was admitted.

An attempt the chain accepted, or one that changed what it must not, is a
success (a defect), and its type stops. So is a lost attempt whose wallet's
nonce then moved. A refusal for another reason is counted apart and fails
the type. An attempt the channel could not make, or one admitted that never
committed, is counted apart and made again. The positive controls: an
in-bounds proposal from a parameter wallet commits before and after the
attempts; where votes run, the vote's proposal reaches its vote and each
repeated voter's first vote counts.

verify recomputes every outcome, count, status and digest from the rows,
which name no address; the evidence binds the genesis files.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import attacks  # noqa: E402
import load  # noqa: E402

SCHEMA = attacks.SCHEMA
FAMILY = 'governance'
PASS, FAIL = attacks.PASS, attacks.FAIL
TARGET = 3000
OUTCOMES = attacks.OUTCOMES
BELOW, OUT_OF_BOUNDS = 'proposal_below_deposit', 'parameter_outside_bounds'
UNBONDED, REPEATED, LATE = 'vote_without_bond', 'repeated_vote', 'vote_after_period'
VOTES = (UNBONDED, REPEATED, LATE)
# The node's rule for each type (governance_store.rs, governance_actions.rs).
# A proposal below the deposit has none: it commits and never opens a vote.
REASONS = {
    BELOW: 'a proposal that never reaches its vote',
    OUT_OF_BOUNDS: 'GOVERNANCE_PARAMETER_OUT_OF_BOUNDS',
    UNBONDED: 'GOVERNANCE_VOTER_NOT_ELIGIBLE',
    REPEATED: 'GOVERNANCE_ALREADY_VOTED',
    LATE: 'GOVERNANCE_VOTING_CLOSED',
}
# A finished proposal is kept one block, then removed: a later vote finds
# no proposal (P01_T04_LATE_VOTES_2026-11-05).
LATE_RULES = ('GOVERNANCE_VOTING_CLOSED', 'GOVERNANCE_PROPOSAL_UNKNOWN')
# CI keeps the production governance periods, where no vote opens.
CI_KINDS = (BELOW, OUT_OF_BOUNDS)
# What a refused attempt's wallet must keep (liquid DGT is checked apart).
HELD = ('bonded_udgt', 'bonds', 'pending_bond_udgt', 'unbonding_udgt', 'unbonds')
# The out-of-bounds values, by k: (CLI flag, the bound's name, side).
PARAMETERS = (('--max-active', 'max_active', 'above'), ('--max-active', 'max_active', 'below'),
              ('--min-self-bond-udgt', 'min_self_bond', 'above'), ('--min-self-bond-udgt', 'min_self_bond', 'below'))
CHOICES = ('yes', 'no', 'no-with-veto', 'abstain')
TRIES = 3
REJECTED = re.compile(r'CheckTx rejected the transaction \((-?\d+)\): (.*)', re.S)


class Failed(Exception):
    pass


digest, rows_bytes = attacks.digest, attacks.rows_bytes


def record_digest(value):
    return digest(attacks.canonical(value))


# Outcomes

def as_named(row):
    """Whether an attempt carried its type's fault, as read from the chain,
    and no other."""
    kind, fault = row['type'], row.get('fault') or {}
    at_next = fault.get('proposal_id') is not None and fault.get('proposal_id') == fault.get('next_proposal_id')
    if kind == BELOW:
        return fault.get('bonded') is True and at_next and 0 <= int(fault['deposit_udgt']) < int(fault['minimum_udgt'])
    if kind == OUT_OF_BOUNDS:
        low, high = fault['bounds']
        return fault.get('bonded') is True and at_next and not low <= int(fault['value']) <= high
    if kind == UNBONDED:
        return fault.get('phase') == 'voting' and fault.get('weight') is None and fault.get('voted') is None
    if kind == REPEATED:
        return fault.get('phase') == 'voting' and fault.get('weight') is not None and fault.get('voted') is not None
    if kind == LATE:
        return (fault.get('weight') is not None and fault.get('voted') is None and fault.get('end_height') is not None
                and int(fault['height']) > int(fault['end_height']))
    return False


def names_rule(kind, log):
    return log in LATE_RULES if kind == LATE else log == REASONS[kind]


def only_fee(state, charges, udgt_change=0):
    """Nothing changed in the wallet but the fees its results charged, one
    nonce for each, and the DGT it was due to move."""
    return (not state['changed'] and state['nonce_step'] == len(charges)
            and int(state['drt_change']) == -sum(charges) and int(state['udgt_change']) == udgt_change)


def below_outcome(row):
    fault, result = row['fault'], row['result']
    if result['code'] != 0:
        return 'refused_other'  # the proposal did not stand: nothing was tested
    deposit, charges = int(fault['deposit_udgt']), [int(result['charge'])]
    if deposit:
        step = row.get('deposit') or {}
        if not isinstance(step.get('code'), int) or isinstance(step.get('code'), bool):
            return 'transport'
        if step['code'] != 0 or (step.get('result') is not None and step['result']['code'] != 0):
            return 'refused_other'
        if step.get('result') is None:
            return 'transport'
        charges.append(int(step['result']['charge']))
    view, state = row.get('proposal_view'), row.get('state')
    if state is None:
        return 'transport'
    if view is None or view['phase'] != 'collecting' or view['deposited_udgt'] != str(deposit):
        return 'admitted'
    refunds = sum(int(r['udgt']) for r in row.get('refunds', []))
    return 'refused' if only_fee(state, charges, refunds - deposit) else 'admitted'


def classify(row):
    """An attempt's outcome from the node's answers and the reads."""
    kind = row['type']
    if kind not in REASONS:
        raise Failed(f'{kind!r} is not an approved governance attack type')
    code = row.get('code')
    if not isinstance(code, int) or isinstance(code, bool):
        return 'transport'
    if not as_named(row):
        return 'refused_other'
    if code != 0:
        return 'refused_other'  # every type passes admission
    result = row.get('result')
    if result is None:
        return 'transport'  # admitted, and expired without a commit
    if kind == BELOW:
        return below_outcome(row)
    if row.get('state') is None:
        return 'transport'
    if result['code'] == 0:
        return 'admitted'
    if result['code'] != 3 or not names_rule(kind, result['log']):
        return 'refused_other'
    if kind in (UNBONDED, REPEATED) and int(result['height']) > int(row['fault']['end_height']):
        return 'refused_other'  # it landed after the vote: another fault
    effect = row.get('effect') or {}
    held = 'before' in effect and effect.get('before') == effect.get('after')
    return 'refused' if only_fee(row['state'], [int(result['charge'])]) and held else 'admitted'


def said(row):
    """What the node said: its admission answer, else the block's result."""
    if row.get('code') not in (None, 0):
        return row.get('log') or ''
    if row['type'] == BELOW and (row.get('result') or {}).get('code') == 0:
        view = row.get('proposal_view')
        if view:
            return f'{view["phase"]} with {view["deposited_udgt"]} of {row["fault"]["minimum_udgt"]} udgt'
    result = row.get('result')
    if result:
        return result['log'] or f'code {result["code"]}'
    return row.get('error') or row.get('log')


def state_facts(before, after):
    """What a row keeps of the reads of its wallet before and after it."""
    return {'changed': sorted(f for f in HELD if before[f] != after[f]),
            'nonce_step': int(after['nonce']) - int(before['nonce']),
            'drt_change': str(int(after['liquid_udrt']) - int(before['liquid_udrt'])),
            'udgt_change': str(int(after['liquid_udgt']) - int(before['liquid_udgt'])),
            'heights': [int(before['context']['height']), int(after['context']['height'])]}


def control_ok(row):
    if row['what'] == 'proposal':
        return (row.get('code') == 0 and (row.get('result') or {}).get('code') == 0 and row.get('stored') is True
                and row.get('in_bounds') is True)
    if row['what'] == 'voting':
        return (row.get('code') == 0 and (row.get('result') or {}).get('code') == 0
                and (row.get('deposit') or {}).get('code') == 0
                and ((row.get('deposit') or {}).get('result') or {}).get('code') == 0 and row.get('phase') == 'voting')
    if row['what'] == 'vote':
        return (row.get('code') == 0 and (row.get('result') or {}).get('code') == 0
                and row.get('choice') is not None and row.get('choice') == row.get('counted'))
    return False


def close_ok(row):
    """A proposal read at or after its close: finished below the minimum, or
    removed while a vote opened at the close would still be running (so it
    never opened one)."""
    if row['read_height'] < row['close_height']:
        return False
    if row['phase'] is None:
        return row['read_height'] <= row['close_height'] + row['voting_period_blocks']
    return row['phase'] == 'finished' and row.get('outcome') == 'below_minimum'


def tally_ok(row):
    return row['before'] == row['after'] or row['admitted'] > 0


def refunds_ok(row):
    """The proposer's liquid DGT between two attempts (or after the last)
    rose by exactly the deposits due back in between."""
    return int(row['udgt_change']) == sum(int(r['udgt']) for r in row['refunds'])


# The run

class Campaign:
    """Each type's attempts against the node, through the desk."""

    def __init__(self, desk, attempts, show=print):
        self.desk, self.n, self.show = desk, attempts, show
        self.rows, self.last, self.ledger = [], {}, None

    def mine(self, kind):
        return [r for r in self.rows if r['type'] == kind]

    def made(self, kind):
        return sum(1 for r in self.mine(kind) if r['outcome'] != 'transport')

    def finished(self, kind):
        mine = self.mine(kind)
        if any(r['outcome'] == 'admitted' for r in mine):
            return True
        running = 0
        for row in reversed(mine):
            if row['outcome'] != 'transport':
                break
            running += 1
        if running >= TRIES * len(self.desk.wallets(kind)):
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

    def closes(self):
        for row in self.desk.closes():
            self.rows.append(row)
            if not close_ok(row):
                self.show(f'== proposal {row["proposal_id"]} at its close: {row["phase"]} {row.get("outcome")}')

    def round(self, batch):
        """[(kind, k, wallet)], each on its own wallet: the CLI's answers,
        each admitted attempt followed to its block, the wallets read
        before and after."""
        desk = self.desk
        labels = [label for _, _, label in batch]
        if len(set(labels)) != len(labels):
            raise Failed('a round has two attempts on one wallet')
        before = desk.accounts(labels)
        self.guard(before)
        context = desk.context(labels)
        plans = [desk.plan(kind, k, label, before[label], context) for kind, k, label in batch]
        answers = desk.submit([(label, args, kind) for (kind, _, label), (args, _) in zip(batch, plans)])
        found = desk.results(answers)
        admitted = any(a.get('code') == 0 for a in answers)
        after = desk.accounts(labels) if admitted else before
        context_after = desk.context(labels) if admitted else context
        stored = (desk.proposer_of(context['next_proposal_id'])
                  if admitted and any(kind == OUT_OF_BOUNDS for kind, _, _ in batch) else None)
        self.last.update({w: int(after[w]['nonce']) for w in labels})
        votes = []
        for (kind, k, label), (_, fault), answer in zip(batch, plans, answers):
            row = {'type': kind, 'k': k, 'wallet': label, 'fault': fault}
            if kind == OUT_OF_BOUNDS:
                fault['proposal_id'] = answer.get('proposal_id')
            if 'code' in answer:
                row.update(code=answer['code'], log=answer['log'][:200])
            else:
                row['error'] = answer['error']
            if answer.get('code') == 0 and found.get(answer['hash']) is not None:
                row['result'] = found[answer['hash']]
                row['state'] = state_facts(before[label], after[label])
                if kind == OUT_OF_BOUNDS:
                    # Its own proposal at the ID it named: none before, none after.
                    row['effect'] = {'what': 'proposal', 'before': False, 'after': desk.is_wallet(label, stored)}
                else:
                    row['effect'] = {'what': 'vote', 'before': context['ballots'][label],
                                     'after': context_after['ballots'][label]}
            row['outcome'] = classify(row)
            self.rows.append(row)
            if kind in VOTES:
                votes.append(row)
        if votes and context['tally'] is not None and context_after['tally'] is not None:
            self.rows.append({'type': 'tally', 'proposal_id': context['vote_id'], 'before': context['tally'],
                              'after': context_after['tally'],
                              'admitted': sum(1 for r in votes if r['outcome'] == 'admitted')})
        self.closes()

    def below(self):
        """One proposal below the deposit: its two steps, the proposer read
        before and after; the refunds due since its last read."""
        desk = self.desk
        label = desk.wallets(BELOW)[0]
        before = desk.accounts([label])
        self.guard(before)
        if self.ledger is not None:
            gap = desk.refunds_row(self.ledger, before[label])
            if gap['refunds'] or gap['udgt_change'] != '0':
                self.rows.append(gap)
        row = desk.below(self.made(BELOW), before[label])
        after = row.pop('_after')
        row['outcome'] = classify(row)
        self.rows.append(row)
        self.last[label] = int(after['nonce'])
        self.ledger = {'height': int(after['context']['height']), 'udgt': int(after['liquid_udgt'])}
        self.closes()

    def control(self, what, when=None):
        row = {'type': 'control', 'what': what, **({'when': when} if when else {}), **self.desk.control(what, when)}
        nonce = row.pop('_nonce', None)
        if nonce is not None:
            self.last[row.pop('_wallet')] = nonce
        self.rows.append(row)
        if not control_ok(row):
            self.show(f'== the positive control {what} {when or ""} failed: {said(row)!r}')
        return row

    def rounds(self, kinds, ready=lambda kind: True):
        """Each kind's next attempts, one a wallet, until each is done (or
        not ready)."""
        while True:
            batch = []
            for kind in kinds:
                if self.finished(kind) or not ready(kind):
                    continue
                made = self.made(kind)
                for n, label in enumerate(self.desk.wallets(kind)[:self.n - made]):
                    batch.append((kind, made + n, label))
            if not batch:
                return
            self.round(batch)

    def run(self, kinds):
        for kind in kinds:
            if kind not in REASONS:
                raise Failed(f'{kind!r} is not an approved governance attack type')
        self.desk.watch = self.closes
        self.control('proposal', 'before')
        if OUT_OF_BOUNDS in kinds:
            self.show(f'== {OUT_OF_BOUNDS}: {self.n} attempts, one a wallet a round')
            self.rounds([OUT_OF_BOUNDS])
        if BELOW in kinds:
            self.show(f'== {BELOW}: {self.n} proposals, one at a time on the whole chain')
            while not self.finished(BELOW):
                self.below()
        self.control('proposal', 'after')
        votes = [k for k in VOTES if k in kinds]
        if votes:
            self.vote(votes)
        if BELOW in kinds:
            self.rows.append(self.desk.refunds_row(self.ledger))
            self.closes()
        for kind in kinds:
            counts = {o: sum(1 for r in self.mine(kind) if r['outcome'] == o) for o in OUTCOMES}
            first = next((said(r) for r in self.mine(kind)), None)
            self.show(f'== {kind}: {counts}; the node said {first!r}')
        return self.rows

    def vote(self, votes):
        """The vote's proposal to its vote, the repeated voters' first
        votes, the votes in the period, then the late votes after it."""
        desk = self.desk
        self.show('== the vote\'s proposal, with the minimum deposit, waits for its vote to open')
        if not control_ok(self.control('voting')):
            return
        for label in desk.wallets(REPEATED) if REPEATED in votes else ():
            self.control('vote', label)
        during = [k for k in votes if k != LATE]
        if during:
            self.show(f'== {", ".join(during)}: {self.n} attempts each, until the vote ends')
            self.rounds(during, desk.in_period)
            for kind in during:
                if self.made(kind) < self.n and not self.finished(kind):
                    self.show(f'== {kind}: the vote ended after {self.made(kind)} of {self.n} attempts')
        if LATE in votes:
            desk.after_period()
            self.show(f'== {LATE}: {self.n} attempts after the vote\'s end')
            self.rounds([LATE])


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


WATCHES = ('control', 'close', 'tally', 'refunds')


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, materials, started, finished):
    for row in rows:
        if row['type'] in WATCHES:
            continue
        # A lost attempt whose wallet's nonce moved counts as admitted.
        lost = row.get('moved_nonce') is True and classify(row) == 'transport' and row['outcome'] == 'admitted'
        if classify(row) != row['outcome'] and not lost:
            raise Failed(f'a {row["type"]} row\'s outcome does not follow from its answers and reads')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    wanted = {('proposal', 'before'), ('proposal', 'after')}
    if materials['vote_kinds']:
        wanted.add(('voting', None))
        if REPEATED in materials['vote_kinds']:
            wanted |= {('vote', label) for label in materials['repeated_voters']}
    watched = {kind: [r for r in rows if r['type'] == kind] for kind in ('close', 'tally', 'refunds')}
    checks = {
        'types': all(t['status'] in (PASS, 'not_run') for t in types.values()),
        'controls': {(r['what'], r.get('when')) for r in controls} == wanted and all(map(control_ok, controls)),
        'closes': all(map(close_ok, watched['close'])),
        'tallies': all(map(tally_ok, watched['tally'])),
        'refunds': all(map(refunds_ok, watched['refunds'])),
    }
    body = {'schema': SCHEMA, 'family': FAMILY, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'started': started, 'finished': finished, 'attempts_wanted': attempts, 'materials': materials,
            'types': types,
            'controls': {'made': len(controls), 'passed': sum(map(control_ok, controls))},
            'closes_read': len(watched['close']),
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

def dgt(udgt):
    """Base units as the CLI's DGT amount (six decimal places)."""
    return f'{udgt // load.UNIT}.{udgt % load.UNIT:06}'


def weight_key(snapshot_height, account_id):
    """The raw state key of an owner's weight in a vote's snapshot
    (governance_store.rs weight_key)."""
    return f'governance:v2:weight:{snapshot_height:020}:{account_id}'.encode().hex()


class Desk:
    """The campaign's wallets on the node. `roles` maps each type to its
    wallets: the proposer (bonded) makes the proposals below the deposit and
    the vote's proposal, the depositor (bonded) its minimum deposit; the
    parameter wallets (bonded voters) the out-of-bounds proposals and the
    late votes; the load wallets (never bonded) the votes without a bond;
    the repeated voters (bonded) vote once, then again."""

    def __init__(self, rpc, cli, env, settings, config, roles, wallets, show=load.say, sleep=time.sleep,
                 clock=time.monotonic):
        self.rpc, self.cli_path, self.env, self.s = rpc, cli, env, settings
        self.show, self.sleep, self.clock = show, sleep, clock
        self.roles, self.by_label = roles, wallets
        governance = config['governance']
        self.minimum = int(governance['deposit']['minimum_deposit_udgt'])
        self.deposit_period = int(governance['deposit']['deposit_period_blocks'])
        self.voting_period = int(governance['ballot']['voting_period_blocks'])
        self.bounds = {name: [int(b['min']), int(b['max'])] for name, b in governance['parameter_bounds'].items()
                       if name in ('max_active', 'min_self_bond')}
        self.max_active = int(config['lifecycle']['max_active'])
        self.gas_price = self.minimum_gas = None
        self.cap = load.GAS_LIMIT * int(config['ordinary']['fee_profile']['gas_price'])
        # Every proposal the campaign made: its close, deposit and depositor.
        self.open, self.checked = {}, set()
        self.scanned, self.vote_info = None, None
        # Called at every poll of a wait: the closes are read as they pass.
        self.watch = None

    # Reads

    def head(self):
        return load.head_height(self.rpc.one('head', 'abci_info', {}))

    def wallets(self, kind):
        return self.roles[kind]

    def accounts(self, labels):
        views = load.queries(self.rpc, 'accounts', [f'/account/{self.by_label[w].address}' for w in labels])
        return dict(zip(labels, views))

    def proposal(self, ident):
        return load.query(self.rpc, 'governance', f'/governance/proposal/{ident}')

    def ballots(self, ident, labels):
        """Each wallet's counted vote on the proposal (None for none, or
        once the proposal is gone)."""
        views = load.queries(self.rpc, 'governance',
                             [f'/governance/vote/{ident}/{self.by_label[w].account_id}' for w in labels])
        return {w: (v or {}).get('choice') for w, v in zip(labels, views)}

    def weight(self, snapshot_height, label):
        value = load.query(self.rpc, 'governance',
                           f'/state/proof/{weight_key(snapshot_height, self.by_label[label].account_id)}')['value']
        return None if value is None else str(int.from_bytes(base64.b64decode(value), 'little'))

    def proposer_of(self, ident):
        view = self.proposal(ident)
        return view and view['proposer']

    def is_wallet(self, label, account_id):
        return account_id is not None and account_id == self.by_label[label].account_id

    def fees(self):
        base = load.query(self.rpc, 'governance', '/ordinary/profile_v3')['fee_profile']['base']
        self.gas_price, self.minimum_gas = int(base['gas_price']), int(base['minimum_gas'])

    def context(self, labels):
        """The next proposal ID; with a vote, its phase, tally and the
        wallets' counted votes."""
        profile = load.query(self.rpc, 'governance', '/ordinary/profile_v3')
        context = {'head': int(profile['context']['height']), 'next_proposal_id': str(profile['next_proposal_id']),
                   'vote_id': None, 'phase': None, 'tally': None, 'ballots': {w: None for w in labels}}
        if self.vote_info:
            ident = self.vote_info['proposal_id']
            view = self.proposal(ident)
            context.update(vote_id=ident, phase=view and view['phase']['phase'],
                           tally=view and view['phase'].get('tally'))
            if view is not None:
                context['ballots'] = self.ballots(ident, labels)
        return context

    # Attempts

    def plan(self, kind, k, label, before, context):
        """The CLI arguments of attempt k and its fault."""
        if kind == OUT_OF_BOUNDS:
            flag, name, side = PARAMETERS[k % len(PARAMETERS)]
            low, high = self.bounds[name]
            step = k // len(PARAMETERS)
            # Below stays in [1, low - 1]: a zero is no bound's test.
            value = high + 1 + step if side == 'above' else low - 1 - step % max(low - 1, 1)
            fault = {'parameter': name, 'value': str(value), 'bounds': [low, high],
                     'bonded': int(before['bonded_udgt']) > 0, 'next_proposal_id': context['next_proposal_id']}
            return ('governance', 'propose', flag, value), fault
        info = self.vote_info
        fault = {'proposal_id': info['proposal_id'], 'phase': context['phase'],
                 'snapshot_height': info['snapshot_height'], 'end_height': info['end_height'],
                 'height': context['head'], 'weight': info['weights'][label],
                 'voted': context['ballots'][label] if kind != LATE else info['ballots'][label]}
        args = ('governance', 'vote', '--proposal-id', info['proposal_id'], '--choice', CHOICES[k % len(CHOICES)])
        return args, fault

    def cli(self, label, args):
        """One write through the CLI's online path: (exit code, stdout,
        stderr)."""
        wallet = self.by_label[label]
        signer = ('--wallet', wallet.keystore_name) if wallet.keystore_name else ('--key-file', str(wallet.key_file))
        with self.rpc.slots:
            result = subprocess.run([str(self.cli_path), *map(str, args), *signer,
                                     '--gas-limit', str(load.GAS_LIMIT), '--maximum-fee-udrt', str(self.cap),
                                     '--expiry-blocks', str(self.s.expiry_blocks), '--wait-seconds', '0'],
                                    capture_output=True, text=True, env=self.env)
        return result.returncode, result.stdout, result.stderr

    def answer(self, label, args):
        """The node's answer to one write: its CheckTx code and log, and for
        an admitted one its engine hash, nonce, expiry and proposal ID. A
        failure before the node answered (the CLI's own checks, the
        channel) is an error, not a refusal."""
        code, out, err = self.cli(label, args)
        if code != 0:
            found = REJECTED.search(err or out or '')
            if found:
                return {'code': int(found.group(1)), 'log': found.group(2).strip()[:200]}
            return {'error': (err.strip() or out.strip())[:200]}
        report = json.loads(out.strip().splitlines()[-1])
        check, body = report['check_tx'], report['body']
        action = body['actions'][0]
        return {'code': int(check['code']), 'log': (check.get('log') or '')[:200],
                'hash': (check.get('engine_hash') or '').lower(), 'nonce': int(body['spending_nonce']),
                'expiry': int(body['expiry_height']),
                'proposal_id': str(action['proposal_id']) if 'proposal_id' in action else None}

    def mark(self):
        """Blocks from the head on may hold what is sent next."""
        self.scanned = max(self.scanned or 0, self.head())

    def submit(self, jobs):
        """[(wallet, args, kind)], each through the CLI at once."""
        self.mark()
        with ThreadPoolExecutor(max_workers=max(1, self.rpc.connections)) as pool:
            return list(pool.map(lambda job: self.answer(job[0], job[1]), jobs))

    def results(self, answers):
        """Each admitted answer's result from the block that holds it: its
        code, rule, gas used, height and charge; None once its expiry
        passed without it."""
        missing = {a['hash']: a['expiry'] for a in answers if a.get('code') == 0}
        found = {}
        while missing:
            head = self.head()
            while self.scanned < head and missing:
                heights = list(range(self.scanned + 1, min(head, self.scanned + load.MAX_BATCH) + 1))
                blocks = self.rpc.call('blocks', [('block', {'height': str(h)}) for h in heights])
                for height, block in zip(heights, blocks):
                    if isinstance(block, load.RpcError):
                        raise block
                    txs = block['block']['data'].get('txs') or []
                    held = {hashlib.sha256(base64.b64decode(tx)).hexdigest(): n for n, tx in enumerate(txs)}
                    mine = [h for h in missing if h in held]
                    if mine:
                        results = self.rpc.one('blocks', 'block_results', {'height': str(height)})['txs_results']
                        for engine_hash in mine:
                            found[engine_hash] = self.result(results[held[engine_hash]], height)
                            del missing[engine_hash]
                self.scanned = heights[-1]
            for engine_hash in [h for h, expiry in missing.items() if head > expiry and self.scanned >= head]:
                found[engine_hash] = None
                del missing[engine_hash]
            if missing:
                self.sleep(self.s.poll_interval)
        return found

    def result(self, result, height):
        gas = int(result.get('gas_used') or 0)
        return {'code': int(result.get('code') or 0), 'log': (result.get('log') or '')[:200], 'gas_used': gas,
                'height': height, 'charge': str(max(gas, self.minimum_gas) * self.gas_price)}

    def one(self, label, args):
        """One write and its result."""
        answer = self.submit([(label, args, None)])[0]
        step = {k: answer[k] for k in ('code', 'log', 'error') if k in answer}
        if answer.get('code') == 0:
            step['result'] = self.results([answer])[answer['hash']]
        return step, answer

    def below(self, k, before):
        """Attempt k of a proposal below the deposit: the proposal, then
        (odd k) its deposit, then the reads. The row keeps the read after
        apart (`_after`) for the next attempt's gap."""
        label = self.wallets(BELOW)[0]
        deposit = 0 if k % 2 == 0 else 1 + k
        next_id = str(load.query(self.rpc, 'governance', '/ordinary/profile_v3')['next_proposal_id'])
        fault = {'deposit_udgt': str(deposit), 'minimum_udgt': str(self.minimum),
                 'bonded': int(before['bonded_udgt']) > 0, 'next_proposal_id': next_id}
        step, answer = self.one(label, ('governance', 'propose', '--max-active', self.max_active))
        fault['proposal_id'] = answer.get('proposal_id')
        row = {'type': BELOW, 'k': k, 'wallet': label, 'fault': fault, **step}
        result = step.get('result')
        if result is not None and result['code'] == 0:
            ident = fault['proposal_id']
            view = self.proposal(ident)
            if view is not None:
                self.open[ident] = {'close': int(view['phase'].get('close_height') or 0), 'udgt': deposit,
                                    'wallet': label}
            if deposit:
                row['deposit'] = self.one(label, ('governance', 'deposit', '--proposal-id', ident,
                                                  '--amount', dgt(deposit)))[0]
            view = self.proposal(ident)
            row['proposal_view'] = view and {'phase': view['phase']['phase'],
                                             'deposited_udgt': str(view['deposited_udgt']),
                                             'close_height': int(view['phase'].get('close_height') or 0),
                                             'admitted_height': int(view['admitted_height'])}
        after = self.accounts([label])[label]
        if result is not None:
            row['state'] = state_facts(before, after)
            low, high = row['state']['heights']
            row['refunds'] = self.refunds(low, high)
        row['_after'] = after
        return row

    def refunds(self, low, high):
        """The deposits due back to the proposer at closes in (low, high]."""
        return [{'proposal_id': ident, 'udgt': str(p['udgt'])} for ident, p in self.made()
                if p['udgt'] and low < p['close'] <= high]

    def made(self):
        return sorted(self.open.items(), key=lambda item: int(item[0]))

    def refunds_row(self, ledger, now=None):
        """The proposer's liquid DGT from its last attempt to `now` (a read):
        it rises by each deposit due back in between."""
        label = self.wallets(BELOW)[0]
        now = now or self.accounts([label])[label]
        high = int(now['context']['height'])
        low = ledger['height'] if ledger else high
        change = int(now['liquid_udgt']) - ledger['udgt'] if ledger else 0
        return {'type': 'refunds', 'heights': [low, high], 'refunds': self.refunds(low, high),
                'udgt_change': str(change)}

    def closes(self):
        """Each proposal the campaign made whose close the head has passed,
        read once."""
        head = self.head()
        rows = []
        for ident, p in self.made():
            if ident in self.checked or p['close'] > head:
                continue
            view = self.proposal(ident)
            phase = view and view['phase']
            rows.append({'type': 'close', 'proposal_id': ident, 'close_height': p['close'],
                         'voting_period_blocks': self.voting_period,
                         'read_height': int(view['context']['height']) if view else head,
                         'phase': phase and phase['phase'], 'outcome': phase and phase.get('outcome'),
                         'deposited_udgt': view and str(view['deposited_udgt'])})
            self.checked.add(ident)
        return rows

    def in_period(self, kind):
        """A vote in the period must land before its end: none is sent once
        its expiry could pass the end."""
        return self.head() + self.s.expiry_blocks + 2 <= self.vote_info['end_height']

    def after_period(self):
        end = self.vote_info['end_height']
        self.until('the vote\'s end', lambda: self.head() > end,
                   self.s.block_time * (max(end - self.head(), 0) + 30))

    def until(self, what, ready, timeout=None):
        deadline = self.clock() + (timeout or self.s.block_time * 30)
        while not ready():
            if self.clock() > deadline:
                raise Failed(f'{what} did not happen in time')
            if self.watch:
                self.watch()
            self.sleep(self.s.poll_interval)

    def control(self, what, when):
        """A write the node should admit: an in-bounds proposal; the vote's
        proposal with the minimum deposit, to its vote; a voter's first
        vote."""
        if what == 'proposal':
            label = self.wallets(OUT_OF_BOUNDS)[0]
            next_id = str(load.query(self.rpc, 'governance', '/ordinary/profile_v3')['next_proposal_id'])
            low, high = self.bounds['max_active']
            step, answer = self.one(label, ('governance', 'propose', '--max-active', self.max_active))
            row = {**step, 'in_bounds': low <= self.max_active <= high, 'stored': False}
            if (step.get('result') or {}).get('code') == 0 and answer.get('proposal_id') == next_id:
                view = self.proposal(next_id)
                row['stored'] = view is not None and view['proposer'] == self.by_label[label].account_id
                if view is not None:
                    self.open[next_id] = {'close': int(view['phase'].get('close_height') or 0), 'udgt': 0,
                                          'wallet': label}
            row.update(_wallet=label, _nonce=int(self.accounts([label])[label]['nonce']))
            return row
        if what == 'voting':
            return self.open_vote()
        step, _ = self.one(when, ('governance', 'vote', '--proposal-id', self.vote_info['proposal_id'],
                                  '--choice', 'yes'))
        counted = self.ballots(self.vote_info['proposal_id'], [when])[when]
        return {**step, 'choice': 'yes', 'counted': counted, '_wallet': when,
                '_nonce': int(self.accounts([when])[when]['nonce'])}

    def open_vote(self):
        """The proposer's no-op proposal and the depositor's minimum deposit;
        then, at the deposit close, the vote's heights and the voters'
        snapshot weights."""
        proposer, depositor = self.roles['proposer'], self.roles['depositor']
        step, answer = self.one(proposer, ('governance', 'propose', '--max-active', self.max_active))
        row = {**step, 'phase': None}
        ident = answer.get('proposal_id')
        if (step.get('result') or {}).get('code') != 0:
            return row
        row['deposit'] = self.one(depositor, ('governance', 'deposit', '--proposal-id', ident,
                                              '--amount', dgt(self.minimum)))[0]
        if ((row['deposit'].get('result') or {}).get('code')) != 0:
            return row
        view = self.proposal(ident)
        close = int(view['phase'].get('close_height') or 0)
        self.until('the vote\'s opening', lambda: (self.proposal(ident) or {}).get('phase', {}).get('phase')
                   != 'collecting', self.s.block_time * (max(close - self.head(), 0) + 30))
        view = self.proposal(ident)
        phase = view and view['phase']
        row['phase'] = phase and phase['phase']
        if row['phase'] != 'voting':
            return row
        voters = [w for kind in VOTES for w in self.roles.get(kind, ())]
        snapshot = int(phase['snapshot_height'])
        self.vote_info = {'proposal_id': ident, 'snapshot_height': snapshot, 'end_height': int(phase['end_height']),
                          'weights': {w: self.weight(snapshot, w) for w in voters},
                          'ballots': self.ballots(ident, voters)}
        return row

    # Setup

    def prepare(self, run, attempts):
        """DRT for the proposer's two writes an attempt and DGT for its
        deposits; the fee profile."""
        proposer = self.by_label[self.roles['proposer']]
        need = (2 * attempts + 10) * self.cap
        short = -(-(need - int(self.accounts([proposer.label])[proposer.label]['liquid_udrt'])) // load.UNIT)
        if short > 0:
            self.show(f'== {run.funder.label} pays the proposer {short} DRT for its attempts\' fee caps')
            run.follow(run.online(run.funder, ('send', '--to', proposer.address, '--amount', str(short),
                                               '--token', 'drt'), kind='send'))
        deposits = sum(1 + k for k in range(1, attempts, 2))
        if deposits:
            amount = -(-deposits // load.UNIT)
            self.show(f'== {run.funder.label} pays the proposer {amount} DGT for its deposits')
            run.follow(run.online(run.funder, ('send', '--to', proposer.address, '--amount', str(amount),
                                               '--token', 'dgt'), kind='send'))
        self.fees()


def materials_of(desk, kinds):
    votes = [k for k in VOTES if k in kinds]
    return {'minimum_udgt': str(desk.minimum), 'deposit_period_blocks': desk.deposit_period,
            'voting_period_blocks': desk.voting_period, 'bounds': desk.bounds, 'max_active': desk.max_active,
            'gas_price': desk.gas_price, 'minimum_gas': desk.minimum_gas, 'vote_kinds': votes,
            'repeated_voters': list(desk.roles.get(REPEATED, ())) if REPEATED in votes else [],
            'vote_proposal_id': desk.vote_info and desk.vote_info['proposal_id']}


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
    # The proposer, depositor and voters bond as their first spend.
    settings.governance = True
    started = load.now_utc()
    gateway = load.Gateway(cli, env, out / 'gateway.log')
    show(f'== the gateway relays to {gateway.endpoint} at {gateway.url}')
    try:
        rpc = load.Rpc(gateway.url, settings.connections)
        run = load.Load(rpc, load.Signer(cli, env, out / 'scratch'), env, cli, fleet, out, settings, funder, validator,
                        config, show=show)
        voters = [w.label for w in run.role('voter')]
        loads = [w.label for w in run.role('load')]
        if len(voters) < 2 or not loads:
            raise Failed('the campaign needs two voters and a load wallet')
        half = len(voters) // 2
        # Each attempt reserves a full fee cap, and fails charged: a wallet
        # takes its share of a type's attempts (the proposer is paid apart).
        share = -(-attempts // min(half, len(voters) - half, len(loads)))
        gas_price = int(config['ordinary']['fee_profile']['gas_price'])
        settings.fund_drt = max(settings.fund_drt, (share + 10) * load.GAS_LIMIT * gas_price)
        roles = {'proposer': run.role('proposer')[0].label, 'depositor': run.role('depositor')[0].label,
                 BELOW: [run.role('proposer')[0].label], OUT_OF_BOUNDS: voters[:half], LATE: voters[:half],
                 REPEATED: voters[half:], UNBONDED: loads}
        desk = Desk(rpc, cli, env, settings, config, roles, run.wallets, show=show)
        run.fund()
        run.first_spends()
        desk.prepare(run, attempts)
        rows = Campaign(desk, attempts, show).run(kinds)
        record = build_record(fleet['chain_id'], native_genesis, engine_genesis, rows, attempts,
                              materials_of(desk, kinds), started, load.now_utc())
        # The rows beside the record, as results.py reads them (FILE.json, FILE.jsonl).
        (out / 'governance.jsonl').write_bytes(rows_bytes(rows))
        (out / 'governance.json').write_text(json.dumps(record, indent=2) + '\n')
        return record
    finally:
        gateway.stop()


def report(record, show=load.say):
    for kind, t in record['types'].items():
        show(f'== {kind}: {t["status"]} {t["counts"]}; first: {(t["first_log"] or "")[:160]!r}')
    show(f'== controls {record["controls"]}, closes read {record["closes_read"]}, successes {record["successes"]}, '
         f'checks {record["checks"]}: {record["status"]}')


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
        print(f'governance: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    sys.exit(main())
