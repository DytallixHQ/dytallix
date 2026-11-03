#!/usr/bin/env python3
"""The upgrade drill (simulation plan, T8): a release handover with its
state upgrade, from the genesis release (RC1) to a second release (RC2), on
the staging network's two hosts, and digest-bound evidence that
`upgrade_drill.py verify` re-evaluates from the file alone.

  upgrade_drill.py verify --evidence FILE --genesis NATIVE_GENESIS

The drill first admits an upgrade and a handover to RC2 and cancels both
before activation (L15, the pre-activation abort), then prepares the
upgrade admission and the handover admission again (target RC2, transition
receipt-index-v1), signs each three of five with the rehearsal kits and
submits them; stages RC2 beside RC1 on both hosts; after the notice,
prepares and signs the upgrade activation and the handover activation
paired with it and submits both, the handover activation first; on each
host in turn, the validator first, waits for RC1 to stop itself and
switches that host to RC2 (switch.sh), the sentry only once the validator
has run alone on RC2 for a while (a late sentry); reconciles before and
after; then returns to RC1 as the upgrade failure runbook's section 6 says:
RC1 staged again, a schema-preserving handover back to it admitted and
activated three of five after the notice, RC2 stopping itself and each host
switched back, and a reconciliation after the return. Each rule is one
check:

- cancelled: the first upgrade and handover were pending, nothing was
  pending after their cancels (the handover's first, then the upgrade's),
  and the chain was read past the cancelled plan's first possible
  activation height (its admission plus the notice) with RC1 still the
  active release and both units still running it;
- admitted: the upgrade and the handover pending, the handover naming RC2;
- notice: the activation's first possible height (its anchor plus one) at
  least the admission height plus the notice (the chain's upgrade policy,
  min_notice_blocks);
- stopped: RC1 stopped itself on each host, refusing because the committed
  release is no longer its own, before that host switched: the validator at the activation (RC1 refuses
  to propose past it), the sentry when the next block reached it from the
  validator on RC2; and the supervisor's final report names the
  application's class, release (E04 gap 15);
- switched: both units run RC2 after switch.sh (the instant of each switch
  is recorded for the event checkpoint after it, T7);
- active: the chain's active release RC2 and active schema 1 (the
  receipt-index migration ran: schema 0 to 1 only through it);
- resumed: both nodes past the activation, the same block and application
  hash at one height after it;
- late_sentry: the sentry switched only once the validator was at least
  LATE_BLOCKS past the halt on RC2, and it caught up past that height;
- host_verify: both hosts verify against their RC2 bundles;
- return_admitted, return_notice, return_stopped, return_switched,
  return_active, return_resumed, return_host_verify: the same rules for the
  return to RC1, whose handover preserves schema 1 and pairs no upgrade;
- reconciled: the reconciliation (T6) PASS before, after and after the return.

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
# Blocks the validator makes alone on RC2 before the sentry switches.
LATE_BLOCKS = 10
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
    notice = observations.get('notice_blocks')

    cancelled = observations.get('cancelled') or {}
    first, after, past = (cancelled.get(k) or {} for k in ('admitted', 'after', 'past'))
    since = admitted_height(first.get('handover_pending'))
    if not (first and after and past and source and target) or not isinstance(notice, int) or since is None \
            or not isinstance(past.get('height'), int):
        add('cancelled', INCONCLUSIVE, 'no cancelled admission, its cancels and a reading past it (L15)')
    else:
        problems = []
        if not first.get('upgrade_pending') or target not in json.dumps(first.get('handover_pending')):
            problems.append('the upgrade and the handover to cancel were not both pending')
        if after.get('upgrade_pending') or after.get('handover_pending'):
            problems.append('a plan was still pending after the cancels')
        if after.get('active_release') != source or past.get('active_release') != source:
            problems.append('the active release changed')
        if past['height'] < since + notice:
            problems.append(f'read at {past["height"]}, before the cancelled plan could activate ({since + notice})')
        units = past.get('units') or {}
        if any(units.get(h) != source[:16] for h in HOSTS):
            problems.append(f'a unit left RC1: {units}')
        add('cancelled', FAIL if problems else PASS, '; '.join(problems) if problems else
            f'admitted at {since} and cancelled; at {past["height"]}, past its first activation height, '
            f'{source[:16]} still active on both hosts')

    def leg(part, frm, to, prefix='', paired=True):
        """One handover: admitted, notice, stopped, switched, active, resumed
        and host_verify, from `frm` to `to`. The forward leg pairs the
        upgrade (the migration); the return is schema-preserving alone."""
        admitted = part.get('admitted') or {}
        upgrade, handover = admitted.get('upgrade_pending'), admitted.get('handover_pending')
        name = lambda check: f'{prefix}{check}'  # noqa: E731
        if not frm or not to or frm == to:
            add(name('admitted'), FAIL, f'the drill needs two distinct releases: {releases}')
        elif not handover or (paired and not upgrade):
            add(name('admitted'), INCONCLUSIVE if not admitted else FAIL,
                'the upgrade or the handover was not pending' if paired else 'the handover was not pending')
        elif to not in json.dumps(handover):
            add(name('admitted'), FAIL, f'the pending handover does not name {to[:16]}')
        else:
            add(name('admitted'), PASS, f'{"the upgrade and " if paired else ""}the handover to {to[:16]} pending')

        # The activation commits after its anchor, so its window's first
        # height bounds the activation height from below.
        since = admitted_height(handover)
        anchor = part.get('activation_anchor_height')
        if not isinstance(notice, int) or since is None or not isinstance(anchor, int):
            add(name('notice'), INCONCLUSIVE, 'no admission height, notice or activation anchor')
        else:
            add(name('notice'), PASS if anchor + 1 >= since + notice else FAIL,
                f'admitted at {since}, activation anchored at {anchor}, notice {notice} blocks')

        # The release stopping itself: its unit stops, it refuses the
        # committed release, and the supervisor's final report names the
        # application's class, release (E04 gap 15).
        stopped = part.get('stopped') or {}
        if not all(isinstance(stopped.get(h), dict) for h in HOSTS):
            add(name('stopped'), INCONCLUSIVE, 'not every host was read after the activation')
        else:
            bad = [f'{h}: {stopped[h]}' for h in HOSTS
                   if stopped[h].get('active') not in ('inactive', 'failed') or not stopped[h].get('release_refusal')
                   or stopped[h].get('application_failure_class') != 'release']
            add(name('stopped'), FAIL if bad else PASS, '; '.join(bad) if bad else
                f'{frm[:16]} stopped itself on both hosts, refusing the committed release (class release)')

        units = part.get('units_after') or {}
        if not all(h in units for h in HOSTS) or not to:
            add(name('switched'), INCONCLUSIVE, 'the units were not read after the switch')
        else:
            wrong = [f'{h} runs {units[h]}' for h in HOSTS if units[h] != to[:16]]
            add(name('switched'), FAIL if wrong else PASS, '; '.join(wrong) if wrong else f'both units run {to[:16]}')

        active = part.get('active') or {}
        if not active:
            add(name('active'), INCONCLUSIVE, 'the status view was not read after the switch')
        else:
            problems = []
            if active.get('release') != to:
                problems.append(f'the active release is {active.get("release")}')
            if active.get('handover_schema') != MIGRATED_SCHEMA or active.get('upgrade_schema') != MIGRATED_SCHEMA:
                problems.append(f'the active schema is {active.get("handover_schema")}/{active.get("upgrade_schema")}')
            add(name('active'), FAIL if problems else PASS, '; '.join(problems) if problems else
                f'release {to[:16]} active, schema {MIGRATED_SCHEMA}: '
                + ('the migration ran' if paired else 'the schema preserved'))

        compared = part.get('compared') or {}
        nodes = [compared.get(n) for n in ('validator', 'sentry')]
        halted = part.get('halt_height')
        if not isinstance(halted, int) or not all(isinstance(n, dict) and n.get('height') for n in nodes):
            add(name('resumed'), INCONCLUSIVE, 'the nodes were not compared after the activation')
        elif nodes[0] != nodes[1]:
            add(name('resumed'), FAIL, f'the nodes differ: validator {nodes[0]}, sentry {nodes[1]}')
        elif nodes[0]['height'] < halted + AFTER:
            add(name('resumed'), FAIL, f'compared at {nodes[0]["height"]}, not {AFTER} blocks past {halted}')
        else:
            add(name('resumed'), PASS, f'the same block and application hash at {nodes[0]["height"]}, past {halted}')

        verified = part.get('host_verify_exit') or {}
        if not all(h in verified for h in HOSTS):
            add(name('host_verify'), INCONCLUSIVE, 'not run on every host')
        else:
            failed = [f'{h} exited {verified[h]}' for h in HOSTS if verified[h] != 0]
            add(name('host_verify'), FAIL if failed else PASS, '; '.join(failed) if failed else
                f'both hosts verify against their {to[:16]} bundles')
        return halted, nodes

    halted, nodes = leg(observations, source, target)

    late = (observations.get('late_sentry') or {}).get('validator_height')
    if not isinstance(halted, int) or not isinstance(late, int) or not isinstance(nodes[1], dict) \
            or not isinstance(nodes[1].get('height'), int):
        add('late_sentry', INCONCLUSIVE, 'no reading of the validator when the sentry switched')
    elif late < halted + LATE_BLOCKS:
        add('late_sentry', FAIL, f'the sentry switched with the validator at {late}, under {LATE_BLOCKS} blocks '
                                 f'past the halt at {halted}')
    elif nodes[1]['height'] <= late:
        add('late_sentry', FAIL, f'the sentry was compared at {nodes[1]["height"]}, not past {late}')
    else:
        add('late_sentry', PASS, f'the sentry switched {late - halted} blocks after the halt and caught up to '
                                 f'{nodes[1]["height"]}')

    # The return (upgrade-failure.md, section 6): a schema-preserving
    # handover back to the previous release.
    leg(observations.get('returned') or {}, target, source, 'return_', paired=False)

    reconciled = {when: observations.get(f'reconcile_{when}') or {} for when in ('before', 'after', 'returned')}
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
