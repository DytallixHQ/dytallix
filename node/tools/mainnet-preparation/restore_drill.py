#!/usr/bin/env python3
"""The restore drill (disaster recovery v1, R6): the sentry is lost, wiped,
reinstalled from its bundle and restored from its off-host copy, then
catches up from the validator. The drill writes digest-bound evidence that
`restore_drill.py verify` re-evaluates from the file alone.

  restore_drill.py verify --evidence FILE --genesis NATIVE_GENESIS

The evidence records the drill's instants on one clock (declared, wiped,
installed, restored, caught up), the copy (its height, SHA-256 and parts),
both nodes' block and application hash at the height they were compared, and
the restored host's `host_install.py verify`. Each rule is one check:

- the order: wiped, then installed, then restored, then caught up;
- the copy: the restored node reached at least the copy's height;
- state: the restored sentry and the validator hold the same block and
  application hash at one height, at or past the copy;
- the restored host verifies against its bundle;
- the time from declared to caught up within the approved sentry recovery
  objective (launch/operations/OBJECTIVES.json, recovery.rto_minutes.sentry:
  4 hours).

Any broken rule is FAIL; a missing reading is INCONCLUSIVE, which no run
counts as a pass. Only PASS exits 0. The target the record was built with is
covered by the digest, and `verify` refuses one that is not the approved
objective. The caught-up instant is the end the checkpoint recorder (T7)
reads for the event checkpoint after a restore (R02).
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
OBJECTIVES = HERE.parents[2] / 'launch' / 'operations' / 'OBJECTIVES.json'
SCHEMA = 'dytallix.restore-drill.v1'
PASS, FAIL, INCONCLUSIVE = 'PASS', 'FAIL', 'INCONCLUSIVE'
# The drill's instants, in the order the runbook requires them.
STEPS = ('declared_at', 'wiped_at', 'installed_at', 'restored_at', 'caught_up_at')


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def approved_target(path=OBJECTIVES):
    """The approved sentry recovery objective, in seconds."""
    return int(json.loads(Path(path).read_bytes())['recovery']['rto_minutes']['sentry']) * 60


def evaluate(observations, target):
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
            'wiped, installed, restored from the copy, then caught up')

    copy = observations.get('copy') or {}
    compared = observations.get('compared') or {}
    nodes = [compared.get(n) for n in ('validator', 'sentry')]
    height = copy.get('height')
    if not isinstance(height, int) or not all(isinstance(n, dict) and isinstance(n.get('height'), int) for n in nodes):
        add('copy', INCONCLUSIVE, 'no copy height or no comparison after the restore')
        add('state', INCONCLUSIVE, 'the nodes were not compared after the restore')
    else:
        add('copy', PASS if nodes[1]['height'] >= height else FAIL,
            f'restored from the copy of height {height}; the sentry reached {nodes[1]["height"]}')
        if nodes[0] != nodes[1]:
            add('state', FAIL, f'the nodes differ: validator {nodes[0]}, sentry {nodes[1]}')
        else:
            add('state', PASS, f'the same block and application hash at {nodes[0]["height"]}')

    verified = observations.get('host_verify_exit')
    add('host_verify', INCONCLUSIVE if verified is None else PASS if verified == 0 else FAIL,
        'not run' if verified is None else f'host_install.py verify exited {verified}')

    if missing:
        add('objective', INCONCLUSIVE, 'the restore time needs every instant')
    else:
        took = instants['caught_up_at'] - instants['declared_at']
        add('objective', PASS if took <= target else FAIL,
            f'{took:.0f} s from declared to caught up; the objective is {target} s')
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
        print(f'restore_drill: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
