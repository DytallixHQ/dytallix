#!/usr/bin/env python3
"""The upgrade drill (simulation plan, T8): a release handover with its
state upgrade, from the genesis release (RC1) to a second release (RC2), on
the staging network's two hosts, and digest-bound evidence that
`upgrade_drill.py verify` re-evaluates from the file alone.

  upgrade_drill.py verify --evidence FILE --genesis NATIVE_GENESIS

The drill prepares the upgrade admission and the handover admission (target
RC2, transition receipt-index-v1), signs each three of five with the
rehearsal kits and submits them; stages RC2 beside RC1 on both hosts; after
the notice, prepares and signs the upgrade activation and the handover
activation paired with it and submits both, the handover activation first;
on each host in turn, the validator first, waits for RC1 to stop itself and
switches that host to RC2 (switch.sh); and reconciles before and after.
Each rule is one check:

- admitted: the upgrade and the handover pending, the handover naming RC2;
- notice: the activation's first possible height (its anchor plus one) at
  least the admission height plus the notice (the chain's upgrade policy,
  min_notice_blocks);
- stopped: RC1 stopped itself on each host, refusing because the committed
  release is no longer its own, before that host switched: the validator at the activation (RC1 refuses
  to propose past it), the sentry when the next block reached it from the
  validator on RC2;
- switched: both units run RC2 after switch.sh (the instant of each switch
  is recorded for the event checkpoint after it, T7);
- active: the chain's active release RC2 and active schema 1 (the
  receipt-index migration ran: schema 0 to 1 only through it);
- resumed: both nodes past the activation, the same block and application
  hash at one height after it;
- host_verify: both hosts verify against their RC2 bundles;
- reconciled: the reconciliation (T6) PASS before and after.

Any broken rule is FAIL; a missing reading is INCONCLUSIVE, which no run
counts as a pass. Only PASS exits 0.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

SCHEMA = 'dytallix.upgrade-drill.v1'
PASS, FAIL, INCONCLUSIVE = 'PASS', 'FAIL', 'INCONCLUSIVE'
HOSTS = ('validator-1', 'sentry-1')
# Heights past the activation both nodes must reach.
AFTER = 3
# The schema the receipt-index migration (receipt-index-v1) moves to.
MIGRATED_SCHEMA = 1


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def admitted_height(pending):
    """The admission height of a pending plan, wherever the view nests it."""
    if isinstance(pending, dict):
        if isinstance(pending.get('admitted_height'), int):
            return pending['admitted_height']
        for value in pending.values():
            found = admitted_height(value)
            if found is not None:
                return found
    return None


def evaluate(observations):
    rows = []

    def add(name, status, detail):
        rows.append({'check': name, 'status': status, 'detail': detail})

    releases = observations.get('releases') or {}
    source, target = releases.get('source'), releases.get('target')
    admitted = observations.get('admitted') or {}
    upgrade, handover = admitted.get('upgrade_pending'), admitted.get('handover_pending')
    if not source or not target or source == target:
        add('admitted', FAIL, f'the drill needs two distinct releases: {releases}')
    elif not upgrade or not handover:
        add('admitted', INCONCLUSIVE if not admitted else FAIL, 'the upgrade or the handover was not pending')
    elif target not in json.dumps(handover):
        add('admitted', FAIL, 'the pending handover does not name the target release')
    else:
        add('admitted', PASS, f'the upgrade and the handover to {target[:16]} pending')

    # The activation commits after its anchor, so its window's first height
    # bounds the activation height from below.
    notice = observations.get('notice_blocks')
    since = admitted_height(handover)
    anchor = observations.get('activation_anchor_height')
    halted = observations.get('halt_height')
    if not isinstance(notice, int) or since is None or not isinstance(anchor, int):
        add('notice', INCONCLUSIVE, 'no admission height, notice or activation anchor')
    else:
        add('notice', PASS if anchor + 1 >= since + notice else FAIL,
            f'admitted at {since}, activation anchored at {anchor}, notice {notice} blocks')

    stopped = observations.get('stopped') or {}
    if not all(isinstance(stopped.get(h), dict) for h in HOSTS):
        add('stopped', INCONCLUSIVE, 'not every host was read after the activation')
    else:
        bad = [f'{h}: {stopped[h]}' for h in HOSTS
               if stopped[h].get('active') not in ('inactive', 'failed') or not stopped[h].get('release_refusal')]
        add('stopped', FAIL if bad else PASS, '; '.join(bad) if bad else
            'RC1 stopped itself on both hosts, refusing the committed release')

    units = observations.get('units_after') or {}
    if not all(h in units for h in HOSTS) or not target:
        add('switched', INCONCLUSIVE, 'the units were not read after the switch')
    else:
        wrong = [f'{h} runs {units[h]}' for h in HOSTS if units[h] != target[:16]]
        add('switched', FAIL if wrong else PASS, '; '.join(wrong) if wrong else f'both units run {target[:16]}')

    active = observations.get('active') or {}
    if not active:
        add('active', INCONCLUSIVE, 'the status view was not read after the switch')
    else:
        problems = []
        if active.get('release') != target:
            problems.append(f'the active release is {active.get("release")}')
        if active.get('handover_schema') != MIGRATED_SCHEMA or active.get('upgrade_schema') != MIGRATED_SCHEMA:
            problems.append(f'the active schema is {active.get("handover_schema")}/{active.get("upgrade_schema")}')
        add('active', FAIL if problems else PASS, '; '.join(problems) if problems else
            f'release {target[:16]} active, schema {MIGRATED_SCHEMA}: the migration ran')

    compared = observations.get('compared') or {}
    nodes = [compared.get(n) for n in ('validator', 'sentry')]
    if not isinstance(halted, int) or not all(isinstance(n, dict) and n.get('height') for n in nodes):
        add('resumed', INCONCLUSIVE, 'the nodes were not compared after the activation')
    elif nodes[0] != nodes[1]:
        add('resumed', FAIL, f'the nodes differ: validator {nodes[0]}, sentry {nodes[1]}')
    elif nodes[0]['height'] < halted + AFTER:
        add('resumed', FAIL, f'compared at {nodes[0]["height"]}, not {AFTER} blocks past {halted}')
    else:
        add('resumed', PASS, f'the same block and application hash at {nodes[0]["height"]}, past {halted}')

    verified = observations.get('host_verify_exit') or {}
    if not all(h in verified for h in HOSTS):
        add('host_verify', INCONCLUSIVE, 'not run on every host')
    else:
        failed = [f'{h} exited {verified[h]}' for h in HOSTS if verified[h] != 0]
        add('host_verify', FAIL if failed else PASS, '; '.join(failed) if failed else
            'both hosts verify against their RC2 bundles')

    reconciled = {when: observations.get(f'reconcile_{when}') or {} for when in ('before', 'after')}
    if not all(r.get('status') for r in reconciled.values()):
        add('reconciled', INCONCLUSIVE, 'no reconciliation before and after')
    else:
        statuses = {when: r['status'] for when, r in reconciled.items()}
        add('reconciled', PASS if set(statuses.values()) == {PASS} else FAIL,
            ', '.join(f'{when} {r["status"]} at {r.get("height")}' for when, r in reconciled.items()))
    found = {r['status'] for r in rows}
    status = FAIL if FAIL in found else INCONCLUSIVE if INCONCLUSIVE in found else PASS
    return rows, status


def build(chain_id, genesis_bytes, observations):
    rows, status = evaluate(observations)
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'genesis': {'native_genesis_sha256': digest(genesis_bytes)},
            'observations': observations, 'checks': rows, 'status': status}
    return dict(body, sha256=digest(canonical(body)))


def verify(record, genesis_bytes):
    """Re-evaluate a record from its observations; any edit or another
    genesis shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if body['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    rows, status = evaluate(body['observations'])
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
        print(f'upgrade_drill: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
