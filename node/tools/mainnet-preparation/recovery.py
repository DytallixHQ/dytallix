#!/usr/bin/env python3
"""The validator recovery drill (simulation plan, T9): the validator's host
is lost and replaced by the fence-first procedure (validator recovery, F17;
node/docs/operations/validator-recovery.md), and the drill writes
digest-bound evidence that `recovery.py verify` re-evaluates from the file
alone.

  recovery.py verify --evidence FILE --genesis NATIVE_GENESIS

The evidence records the procedure's instants on one clock (declared,
fenced, the sentry's consensus log set aside, installed, the sentry and then
the validator started, the first block after the halt), the halt height H
read twice from the sentry, the heights both nodes reached after it, their
block and application hashes at one height after it, the validator set the
client's status view lists before and after, and the replacement's
`host_install.py verify`. Each rule of the procedure is one check:

- the order: nothing installed before the old host was fenced, and the
  validator started only after the sentry's old votes were set aside and the
  sentry was started;
- the halt confirmed: the sentry's height read twice, unchanged;
- signing again: the validator past H + 1 and the sentry following, the same
  block and application hash on both at a height after H;
- no double sign: the same validator set before and after (with one
  validator, a double sign removes it and the chain cannot continue);
- the replacement verifies against its bundle;
- the time from declared to the first block after H within the approved
  validator recovery objective (launch/operations/OBJECTIVES.json,
  recovery.rto_minutes.validator: 8 hours).

Any broken rule is FAIL; a missing reading is INCONCLUSIVE, which no run
counts as a pass. Only PASS exits 0. The target the record was built with is
covered by the digest, and `verify` refuses one that is not the approved
objective.

The lean scope is the CI drill on the staging network (`staging_network.py
validator-recovery-drill`), where the runner's wipe.sh stands for the
provider deleting the old server. On the real hosts each step is at a
console (install types the seal code), so the founder records the instants
there; recording them through public paths follows.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
OBJECTIVES = HERE.parents[2] / 'launch' / 'operations' / 'OBJECTIVES.json'
SCHEMA = 'dytallix.validator-recovery.v1'
PASS, FAIL, INCONCLUSIVE = 'PASS', 'FAIL', 'INCONCLUSIVE'
# The procedure's instants, in the order the procedure requires them.
STEPS = ('declared_at', 'fenced_at', 'wal_set_aside_at', 'installed_at', 'sentry_started_at',
         'validator_started_at', 'first_block_after_halt_at')
# Heights past H both nodes must reach: signing again, not one block.
AFTER = 3


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def approved_target(path=OBJECTIVES):
    """The approved validator recovery objective, in seconds."""
    return int(json.loads(Path(path).read_bytes())['recovery']['rto_minutes']['validator']) * 60


def evaluate(observations, target):
    """The checks, each (name, status, detail), from the observations."""
    rows = []

    def add(name, status, detail):
        rows.append({'check': name, 'status': status, 'detail': detail})

    instants = observations.get('instants') or {}
    missing = [s for s in STEPS if not is_number(instants.get(s))]
    if missing:
        add('order', INCONCLUSIVE, f'no instant recorded for {", ".join(missing)}')
    else:
        late = [f'{a} after {b}' for a, b in zip(STEPS, STEPS[1:]) if instants[a] > instants[b]]
        add('order', FAIL if late else PASS, '; '.join(late) if late else
            'fenced before the install; the old votes set aside and the sentry started before the validator')

    halt = observations.get('halt_readings') or []
    if len(halt) < 2 or not all(isinstance(h, int) for h in halt):
        add('halt', INCONCLUSIVE, 'the sentry\'s height was not read twice after the fence')
    else:
        add('halt', PASS if len(set(halt)) == 1 else FAIL,
            f'the sentry stayed at {halt[0]}' if len(set(halt)) == 1 else f'the sentry moved after the fence: {halt}')
    height = halt[-1] if halt and isinstance(halt[-1], int) else None

    reached = observations.get('reached') or {}
    if height is None or not all(isinstance(reached.get(n), int) for n in ('validator', 'sentry')):
        add('signing', INCONCLUSIVE, 'no heights after the halt for both nodes')
    else:
        short = [f'{n} at {reached[n]}' for n in ('validator', 'sentry') if reached[n] < height + AFTER]
        add('signing', FAIL if short else PASS, f'not {AFTER} blocks past H={height}: {", ".join(short)}' if short
            else f'the validator at {reached["validator"]} and the sentry at {reached["sentry"]}, past H={height}')

    compared = observations.get('compared') or {}
    nodes = [compared.get(n) for n in ('validator', 'sentry')]
    if height is None or not all(isinstance(n, dict) and n.get('height') for n in nodes):
        add('state', INCONCLUSIVE, 'the nodes were not compared at a height after the halt')
    elif nodes[0] != nodes[1]:
        add('state', FAIL, f'the nodes differ: validator {nodes[0]}, sentry {nodes[1]}')
    elif nodes[0]['height'] <= height:
        add('state', INCONCLUSIVE, f'compared at {nodes[0]["height"]}, not after H={height}')
    else:
        add('state', PASS, f'the same block and application hash at {nodes[0]["height"]}')

    before, after = observations.get('validators_before'), observations.get('validators_after')
    if not before or after is None:
        add('validator_set', INCONCLUSIVE, 'the validator set was not read before and after')
    else:
        add('validator_set', PASS if before == after else FAIL,
            f'{len(before)} validator(s), unchanged' if before == after else
            f'the validator set changed: {before} before, {after} after')

    verified = observations.get('host_verify_exit')
    add('host_verify', INCONCLUSIVE if verified is None else PASS if verified == 0 else FAIL,
        'not run' if verified is None else f'host_install.py verify exited {verified}')

    if missing:
        add('objective', INCONCLUSIVE, 'the recovery time needs every instant')
    else:
        took = instants['first_block_after_halt_at'] - instants['declared_at']
        add('objective', PASS if took <= target else FAIL,
            f'{took:.0f} s from declared to the first block after the halt; the objective is {target} s')
    found = {r['status'] for r in rows}
    status = FAIL if FAIL in found else INCONCLUSIVE if INCONCLUSIVE in found else PASS
    return rows, status


def build(chain_id, genesis_bytes, observations, target):
    rows, status = evaluate(observations, target)
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'genesis': {'native_genesis_sha256': digest(genesis_bytes)},
            'target_seconds': target, 'observations': observations, 'checks': rows, 'status': status}
    return dict(body, sha256=digest(canonical(body)))


def verify(record, genesis_bytes, target=None):
    """Re-evaluate a record from its observations; any edit, another genesis
    or a target that is not the approved objective shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if body['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    target = approved_target() if target is None else target
    if body['target_seconds'] != target:
        raise Failed(f'the record was judged against {body["target_seconds"]} s, not the approved {target} s')
    rows, status = evaluate(body['observations'], target)
    if (rows, status) != (body['checks'], body['status']):
        raise Failed('the recorded checks do not follow from the recorded observations')
    return status


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True, help='native-genesis.json')
    args = parser.parse_args()
    try:
        status = verify(json.loads(args.evidence.read_bytes()), args.genesis.read_bytes())
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'recovery: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
