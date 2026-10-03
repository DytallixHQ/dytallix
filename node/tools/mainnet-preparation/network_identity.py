#!/usr/bin/env python3
"""Check genesis records against the approved network identity (D13-Q01). Accepts nothing.

The identity is launch/genesis/IDENTITY.json (P01, 3 October 2026): chain ID
dytallix-mainnet-1, network mainnet, display name Dytallix, and a genesis time
set at the final freeze, in whole seconds UTC, at 14:00:00 on a weekday and at
least 72 hours after the final build.

  network_identity.py earliest --built-at 2027-01-04T09:30:00Z
  network_identity.py check --genesis-time 2027-01-07T14:00:00Z --built-at 2027-01-04T09:30:00Z

`earliest` prints the first genesis time the procedure allows after a build;
`check` exits 2 with the reasons when a genesis time breaks it. A missed
genesis time is never reused: set a new one and rebuild.
"""
import argparse
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import re
import sys

IDENTITY = Path(__file__).resolve().parents[3]/'launch'/'genesis'/'IDENTITY.json'
SCHEMA = 'dytallix.network-identity.v1'
UTC_SECONDS = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$')
WEEKDAYS = ('Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday')


def load(path=IDENTITY):
    identity = json.loads(Path(path).read_text())
    if identity.get('schema') != SCHEMA: raise ValueError('network identity schema mismatch')
    rule = identity['genesis_time']
    if rule['value'] is not None: raise ValueError('the genesis time is set at the final freeze, not in the identity record')
    if not set(rule['weekdays']) <= set(WEEKDAYS) or rule['min_lead_hours'] < 0: raise ValueError('invalid genesis time rule')
    return identity


def instant(value, what):
    """A whole-second UTC time as the genesis builder writes it."""
    if not isinstance(value, str) or UTC_SECONDS.fullmatch(value) is None:
        raise ValueError(f'{what} must be whole seconds in UTC, as YYYY-MM-DDTHH:MM:SSZ')
    return datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)


def text(moment): return moment.strftime('%Y-%m-%dT%H:%M:%SZ')


def time_errors(identity, genesis_time, built_at=None):
    """Reasons a genesis time breaks the approved procedure; built_at adds the lead check."""
    rule = identity['genesis_time']
    try: moment = instant(genesis_time, 'genesis_time')
    except ValueError as exc: return [str(exc)]
    errors = []
    if moment.strftime('%H:%M:%S') != rule['time_of_day_utc']:
        errors.append(f'genesis_time must be at {rule["time_of_day_utc"]} UTC')
    if WEEKDAYS[moment.weekday()] not in rule['weekdays']:
        errors.append('genesis_time must fall on ' + ', '.join(rule['weekdays']))
    if built_at is not None:
        try: built = instant(built_at, 'built_at')
        except ValueError as exc: return errors + [str(exc)]
        if moment - built < timedelta(hours=rule['min_lead_hours']):
            errors.append(f'genesis_time must be at least {rule["min_lead_hours"]} hours after the final build')
    return errors


def earliest(identity, built_at):
    """The first genesis time the procedure allows after a build at built_at."""
    rule = identity['genesis_time']
    hour, minute, second = (int(x) for x in rule['time_of_day_utc'].split(':'))
    floor = instant(built_at, 'built_at') + timedelta(hours=rule['min_lead_hours'])
    moment = floor.replace(hour=hour, minute=minute, second=second)
    if moment < floor: moment += timedelta(days=1)
    while WEEKDAYS[moment.weekday()] not in rule['weekdays']: moment += timedelta(days=1)
    return text(moment)


def record_errors(identity, records):
    """Reasons genesis records differ from the approved identity. The genesis
    time's lead is checked at the freeze with `check --built-at`."""
    errors = []
    if records.get('chain_id') != identity['chain_id']:
        errors.append(f'chain_id is not the approved {identity["chain_id"]}')
    if records.get('network') != identity['network']:
        errors.append(f'network is not the approved {identity["network"]}')
    return errors + time_errors(identity, records.get('genesis_time'))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    first = commands.add_parser('earliest', help='the first genesis time allowed after a build')
    first.add_argument('--built-at', required=True)
    check = commands.add_parser('check', help='check a genesis time against the procedure')
    check.add_argument('--genesis-time', required=True)
    check.add_argument('--built-at', required=True)
    args = parser.parse_args()
    try:
        identity = load()
        if args.command == 'earliest':
            print(json.dumps({'chain_id': identity['chain_id'], 'genesis_time': earliest(identity, args.built_at)}))
            return 0
        errors = time_errors(identity, args.genesis_time, args.built_at)
    except (OSError, ValueError, KeyError) as exc:
        print(f'INVALID: {exc}', file=sys.stderr); return 2
    print(json.dumps({'status': 'FOLLOWS_PROCEDURE' if not errors else 'BREAKS_PROCEDURE', 'errors': errors}))
    return 0 if not errors else 2


if __name__ == '__main__': raise SystemExit(main())
