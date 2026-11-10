#!/usr/bin/env python3
"""The staging hosts record (staging.hosts.ready; P01, 10 November 2026,
launch/approvals/P01_T06_STAGING_HOSTS_EVIDENCE_2026-11-10.json): the
dytallix-staging-2 install counts, and each host is verified after T9, the
validator by T9's own host_install.py verify and the sentry by a
host_install.py verify at its console.

  staging_hosts.py build --staging chain/STAGING.json --genesis NATIVE_GENESIS \
      --recovery T9_EVIDENCE.json --observations OBSERVATIONS.json --out FILE
  staging_hosts.py verify --evidence FILE --staging chain/STAGING.json \
      --genesis NATIVE_GENESIS --recovery T9_EVIDENCE.json

The record binds the chain's STAGING.json (its chain and its two bundles'
SHA-256 and roles), the native genesis and the T9 validator recovery record
(recovery.py; its digest, and its host_verify reading, which is the
validator's verify). The founder's observations hold the rest:

  {"verified_at": ISO instant of the sentry's console verify,
   "countries": {"validator": "DE", "sentry": "FI"},       ISO 3166 alpha-2
   "sentry_verify_exit": 0,
   "sentry_bundle_tar_sha256": null or the tar's SHA-256 if still on the host,
   "public": {"sentry_status_page": true, "sentry_client_channel": true,
              "validator_no_public_path": true}}

Countries only: never a region, an address, a host name or a server ID (the
record's digest is published; the record itself stays in the run directory).
Each rule is one check: one validator and one sentry bundle; each host's
verify exited 0; the sentry's tar, if read, is its bundle; the countries
differ; the sentry answers on its status page and client channel and the
validator on no public path. Any broken rule is FAIL; a missing reading is
INCONCLUSIVE. Only PASS exits 0. results.py summarize staging reduces the
record to staging.hosts.ready: the count of hosts whose verify exited 0.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import recovery  # noqa: E402

SCHEMA = 'dytallix.staging-hosts.v1'
PASS, FAIL, INCONCLUSIVE = recovery.PASS, recovery.FAIL, recovery.INCONCLUSIVE
ROLES = ('validator', 'sentry')
COUNTRY = re.compile(r'[A-Z]{2}')
SHA256 = re.compile(r'[0-9a-f]{64}')
PUBLIC = ('sentry_status_page', 'sentry_client_channel', 'validator_no_public_path')


class Failed(Exception):
    pass


def digest(data): return hashlib.sha256(data).hexdigest()


def canonical(value): return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def bundles(staging):
    """The chain's bundles by role: {role: sha256}, from STAGING.json."""
    found = {}
    for label, bundle in sorted((staging.get('bundles') or {}).items()):
        found.setdefault(bundle.get('role'), []).append(bundle.get('sha256'))
    return found


def validator_exit(recovery_record):
    """The validator's host_install.py verify exit, as T9 recorded it."""
    return recovery_record['observations'].get('host_verify_exit')


def evaluate(body):
    rows = []

    def add(name, status, detail):
        rows.append({'check': name, 'status': status, 'detail': detail})

    roles = body['bundles']
    if sorted(roles) != sorted(ROLES) or any(len(roles[r]) != 1 or not isinstance(roles[r][0], str)
                                             or SHA256.fullmatch(roles[r][0]) is None for r in roles):
        add('bundles', FAIL, f'the chain does not have one validator and one sentry bundle: {sorted(roles)}')
    else:
        add('bundles', PASS, 'one validator and one sentry bundle')
    obs = body['observations']
    exits = {'validator': body['recovery']['host_verify_exit'], 'sentry': obs.get('sentry_verify_exit')}
    for role in ROLES:
        code = exits[role]
        add(f'{role}_verify', INCONCLUSIVE if code is None else PASS if code == 0 else FAIL,
            'not run' if code is None else f'host_install.py verify exited {code}')
    tar = obs.get('sentry_bundle_tar_sha256')
    if tar is not None:
        sentry = roles.get('sentry') or [None]
        add('sentry_bundle', PASS if tar == sentry[0] else FAIL,
            'the tar on the sentry is its bundle' if tar == sentry[0] else 'the tar on the sentry is not its bundle')
    countries = obs.get('countries') or {}
    codes = [countries.get(r) for r in ROLES]
    if not all(isinstance(c, str) and COUNTRY.fullmatch(c) for c in codes):
        add('countries', INCONCLUSIVE, 'a host has no country (ISO 3166 alpha-2)')
    else:
        add('countries', PASS if codes[0] != codes[1] else FAIL,
            'the validator and the sentry are in different countries' if codes[0] != codes[1]
            else 'the validator and the sentry are in the same country')
    public = obs.get('public') or {}
    readings = [public.get(k) for k in PUBLIC]
    if any(r is False for r in readings):
        add('public', FAIL, 'not the one public node: ' + ', '.join(k for k in PUBLIC if public.get(k) is False))
    elif not all(r is True for r in readings):
        add('public', INCONCLUSIVE, 'not read: ' + ', '.join(k for k in PUBLIC if public.get(k) is not True))
    else:
        add('public', PASS, 'the sentry is the one public node')
    found = {r['status'] for r in rows}
    status = FAIL if FAIL in found else INCONCLUSIVE if INCONCLUSIVE in found else PASS
    return rows, status


def verified(record):
    """The hosts whose verify exited 0."""
    checks = ('validator_verify', 'sentry_verify')
    return sum(row['status'] == PASS for row in record['checks'] if row['check'] in checks)


def build(staging, genesis_bytes, recovery_raw, observations):
    record = json.loads(recovery_raw)
    try:
        t9 = recovery.verify(record, genesis_bytes)
    except (recovery.Failed, KeyError, TypeError) as error:
        raise Failed(f'the T9 recovery record does not verify: {error}')
    body = {'schema': SCHEMA, 'chain_id': staging['chain_id'],
            'genesis': {'native_genesis_sha256': digest(genesis_bytes)}, 'bundles': bundles(staging),
            'recovery': {'sha256': digest(recovery_raw), 'status': t9, 'host_verify_exit': validator_exit(record)},
            'observations': observations}
    if record['chain_id'] != body['chain_id']:
        raise Failed('the T9 recovery record names another chain')
    rows, status = evaluate(body)
    body.update(checks=rows, status=status)
    return dict(body, sha256=digest(canonical(body)))


def verify(record, staging, genesis_bytes, recovery_raw):
    """Re-evaluate a record from its inputs; an edit, another chain, bundle,
    genesis or T9 record shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    fresh = build(staging, genesis_bytes, recovery_raw, body['observations'])
    for key, what in (('chain_id', 'another chain'), ('genesis', 'another native genesis'),
                      ('bundles', 'other bundles'), ('recovery', 'another T9 recovery record')):
        if body[key] != fresh[key]:
            raise Failed(f'the evidence names {what}')
    if (body['checks'], body['status']) != (fresh['checks'], fresh['status']):
        raise Failed('the recorded checks do not follow from the recorded observations')
    return body['status']


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('build', 'verify'):
        c = commands.add_parser(name)
        for arg in ('staging', 'genesis', 'recovery'):
            c.add_argument(f'--{arg}', type=Path, required=True)
    commands.choices['build'].add_argument('--observations', type=Path, required=True)
    commands.choices['build'].add_argument('--out', type=Path, required=True)
    commands.choices['verify'].add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    try:
        inputs = (json.loads(args.staging.read_bytes()), args.genesis.read_bytes(), args.recovery.read_bytes())
        if args.command == 'build':
            record = build(*inputs, json.loads(args.observations.read_bytes()))
            args.out.write_text(json.dumps(record, indent=2) + '\n')
            status = record['status']
        else:
            status = verify(json.loads(args.evidence.read_bytes()), *inputs)
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'staging_hosts: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
