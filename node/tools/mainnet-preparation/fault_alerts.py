#!/usr/bin/env python3
"""Fault alerts (simulation plan, T5; campaign.faults.alerts): whether each
injected fault raised its alert and resolved it after recovery. Scope
(P01, 9 November 2026; launch/approvals/P01_T05_FAULT_ALERTS_2026-11-09.json;
alerts, timing and repetitions P01, 10 November 2026,
launch/approvals/P01_T05_FAULT_ALERTS_2026-11-10.json):
the faults past an alert threshold only (the network partition, host loss,
the sentry rebuild and the validator host recovery), in the CI staging
network only, observed by a stand-in alerting service on the runner.

  fault_alerts.py verify --evidence FILE --genesis NATIVE_GENESIS

Each fault's window comes from its own drill's digest-bound evidence: the
fault injector's partition and host loss (faults.py; injected, cleared and
recovered), the restore drill's sentry rebuild (declared to caught up) and
the validator recovery drill's lost validator host (declared to the first
block after the halt). Every alert episode (an alert firing, and its
resolve if one came) the stand-in recorded from either host is paired up
from its events, and those that fired within a window are the fault's
observed alerts, recorded with the time each fired and resolved.

Each fault must raise its decided alerts (EXPECTED; P01, 10 November 2026):
the partition and host loss the sentry's consensus halt and monitoring down
(the alerting service's heartbeat rule) respectively, the sentry rebuild the
sentry's monitoring down, and the validator host the sentry's consensus halt
and the validator's monitoring down. A fault passes when each of its alerts
fired before its recovery (from its injection) and resolved after its
recovery, where a resolve up to one monitor period (RESOLVE_GRACE_SECONDS)
before the recorded recovery counts as after it (P01, 10 November 2026: the
monitor and the reader see the recovery at different moments); it fails
otherwise. A fault whose drill saw no recovery is
INCONCLUSIVE here (its own KPI fails it).
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import faults  # noqa: E402

SCHEMA = 'dytallix.fault-alerts.v1'
PASS, FAIL, INCONCLUSIVE = faults.PASS, faults.FAIL, faults.INCONCLUSIVE
# The in-scope faults (P01, 9 November 2026).
FAULTS = ('partition', 'host_loss', 'sentry_rebuild', 'validator_host')
# A resolve this long before the recorded recovery still counts as after it:
# one monitor period (P01, 10 November 2026).
RESOLVE_GRACE_SECONDS = 60
# Which alerts each fault must raise, as [[host, condition], ...] (P01, 10
# November 2026). Host loss raises the alerting service's heartbeat rule.
EXPECTED = {'partition': [['sentry-1', 'consensus_halt']],
            'host_loss': [['sentry-1', 'monitoring_down']],
            'sentry_rebuild': [['sentry-1', 'monitoring_down']],
            'validator_host': [['sentry-1', 'consensus_halt'], ['validator-1', 'monitoring_down']]}


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def parse_events(raw):
    """The stand-in's events.jsonl; a line still being written is skipped."""
    events = []
    for line in raw.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if isinstance(event, dict) and isinstance(event.get('body'), dict):
            events.append(event)
    return events


def episodes(events):
    """Each alert that fired, in order, with the resolve that followed it for
    the same host and condition (None while still firing)."""
    out, open_ = [], {}
    for event in events:
        body = event['body']
        if event.get('to') not in ('alert', 'service') or body.get('kind', 'change') != 'change':
            continue
        key = (body.get('host'), body.get('condition'))
        if body.get('status') == 'firing' and key not in open_:
            open_[key] = {'host': key[0], 'condition': key[1], 'severity': body.get('severity'),
                          'fired_at': event['received'], 'resolved_at': None}
            out.append(open_[key])
        elif body.get('status') == 'resolved' and key in open_:
            open_.pop(key)['resolved_at'] = event['received']
    return out


def firing(events):
    """(host, condition) of every alert still firing."""
    return [(e['host'], e['condition']) for e in episodes(events) if e['resolved_at'] is None]


def seconds(value):
    return None if value is None else faults.instant(value).timestamp() if isinstance(value, str) else float(value)


def windows(partition_record, host_loss_record, restore_record, recovery_record):
    """Each fault's window, in epoch seconds, from its drill's evidence, and
    that evidence's digest."""
    def scenario(record, name):
        rows = [r for r in record['scenarios'] if r['name'] == name]
        if len(rows) != 1:
            raise Failed(f'the fault record holds {len(rows)} {name} scenarios, not 1')
        row = rows[0]
        return {'evidence_sha256': record['sha256'], 'injected_at': seconds(row.get('injected_at')),
                'cleared_at': seconds(row.get('cleared_at')), 'recovered_at': seconds(row.get('recovered_at'))}

    def drill(record, end):
        instants = record['observations']['instants']
        return {'evidence_sha256': record['sha256'], 'injected_at': instants['declared_at'], 'cleared_at': None,
                'recovered_at': instants.get(end)}

    return {'partition': scenario(partition_record, 'partition'),
            'host_loss': scenario(host_loss_record, 'host_loss'),
            'sentry_rebuild': drill(restore_record, 'caught_up_at'),
            'validator_host': drill(recovery_record, 'first_block_after_halt_at')}


def observed(window, found):
    """The episodes that fired within the window."""
    start, end = window['injected_at'], window['recovered_at']
    return [dict(e) for e in found if start is not None and end is not None and start <= e['fired_at'] <= end]


def evaluate(fault, window, seen, expected):
    """(status, detail) for one fault from its window and observed alerts."""
    if window['injected_at'] is None or window['recovered_at'] is None:
        return INCONCLUSIVE, 'the drill saw no recovery, so no resolve can be judged'
    shown = ', '.join(f'{e["host"]} {e["condition"]}' + (' resolved' if e['resolved_at'] is not None else ' firing')
                      for e in seen) or 'no alert'
    missing = []
    for host, condition in expected:
        if not any(e['host'] == host and e['condition'] == condition and e['resolved_at'] is not None
                   and e['resolved_at'] >= window['recovered_at'] - RESOLVE_GRACE_SECONDS for e in seen):
            missing.append(f'{host} {condition}')
    if missing:
        return FAIL, f'not fired before and resolved after the recovery: {", ".join(missing)}; seen: {shown}'
    return PASS, f'fired and resolved: {shown}'


def run_status(statuses):
    if FAIL in statuses:
        return FAIL
    return PASS if statuses and all(s == PASS for s in statuses) else INCONCLUSIVE


def build_row(fault, window, found, expected):
    seen = observed(window, found)
    status, detail = evaluate(fault, window, seen, expected)
    return dict(window, fault=fault, expected=expected, observed=seen, status=status, detail=detail)


def build(chain_id, genesis_bytes, fault_windows, events_raw, finished):
    """The record over every in-scope fault, from the stand-in's events,
    judged against the decided alerts (EXPECTED)."""
    found = episodes(parse_events(events_raw))
    rows = [build_row(fault, fault_windows[fault], found, EXPECTED[fault]) for fault in FAULTS]
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'genesis': {'native_genesis_sha256': digest(genesis_bytes)},
            'events_sha256': digest(events_raw), 'finished': finished, 'faults': rows,
            'status': run_status([row['status'] for row in rows])}
    body['sha256'] = digest(canonical(body))
    return body


def verify(record, genesis_bytes):
    """Re-evaluate a record from its windows and observed alerts."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    rows = record['faults']
    if [row.get('fault') for row in rows] != list(FAULTS):
        raise Failed(f'the record does not hold exactly the in-scope faults {FAULTS}')
    statuses = []
    for row in rows:
        expected = row.get('expected')
        if expected != EXPECTED[row['fault']]:
            raise Failed(f'{row["fault"]} was judged against other alerts than the decided ones')
        if row['observed'] != observed(row, row['observed']):
            raise Failed(f'{row["fault"]}: an observed alert fired outside its window')
        if (row['status'], row['detail']) != evaluate(row['fault'], row, row['observed'], expected):
            raise Failed(f'{row["fault"]} does not follow from its observations')
        statuses.append(row['status'])
    if run_status(statuses) != record['status']:
        raise Failed('the recorded status does not follow from its faults')
    return record['status']


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True)
    args = parser.parse_args()
    try:
        record = json.loads(args.evidence.read_bytes())
        status = verify(record, args.genesis.read_bytes())
    except (Failed, OSError, KeyError, TypeError, ValueError) as error:
        print(f'fault_alerts: {error}', file=sys.stderr)
        return 1
    for row in record['faults']:
        print(f'{row["status"]:<12} {row["fault"]}: {row["detail"]}')
    print(status)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
