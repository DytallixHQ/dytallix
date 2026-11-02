#!/usr/bin/env python3
"""The results collector (simulation plan, results pipeline; P01, 14 October
and 2 November 2026): it reduces the tools' digest-bound evidence to the
public result summaries the launch page shows, and signs them with the
results key.

  results.py summarize campaign --faults FILE [FILE ...] --genesis NATIVE_GENESIS \
      --engine-genesis ENGINE_GENESIS --release BUILD_RECORD.json --commit SHA [--root DIR]
  results.py sign --cli DYTALLIX --home DIR SUMMARY
  results.py verify --cli DYTALLIX SUMMARY [SUMMARY ...]

A summary (dytallix.launch-result.v1) holds numbers and statuses only: each
KPI's value, its samples, the approved target and the repetitions the plan
asks for; the candidate (its commit and release set digest); and the SHA-256,
chain and status of every evidence file it came from. Nothing else is copied
from the evidence (the fault records hold addresses), and every summary must
pass the launch page's checks before it is written. Campaign summaries are
cumulative: each covers all the evidence to its date, so the newest is the
whole picture. One is written as launch/simulations/results/campaign/DATE.json,
DATE the day of the newest recovery in it.

The first summarizer reads the fault injector's records (T5): the
partition's recovery is campaign.faults.network_heal, judged against the
approved 120 s (TIMINGS.json fault_recovery_target_seconds), and the
validator's process restart is campaign.faults.validator_restart, against
300 s (validator_restart_target_seconds). Each record is re-verified
(faults.verify) against the genesis files and must have been judged against
the approved target for its scenario. A KPI fails once one recovery misses
its target or never happened; it passes once the repetitions are reached (30
per quick fault: this tool's reading of the plan's "about 30 repetitions per
quick fault") and is in progress before that. Its value is the slowest
recovery. Runs, checkpoints, attacks, performance and endurance follow.

sign signs a summary with the results key, whose keystore's active wallet
must be launch-results with the newest public key in RESULTS_KEY.json, over
'dytallix-launch-results-v1:' and the SHA-256 of the summary's exact bytes,
and checks the signature with the CLI before writing SUMMARY.sig. verify
checks a summary against the page's rules and its signature against the keys
RESULTS_KEY.json lists. launch_dashboard.py reads the summaries.
"""
import argparse
from datetime import timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import faults  # noqa: E402
import launch_dashboard  # noqa: E402
import staging_command  # noqa: E402

ROOT = launch_dashboard.ROOT
SCHEMA = launch_dashboard.RESULT_SCHEMA
KEY_SCHEMA = 'dytallix.launch-results-key.v1'
KEYS = 'launch/simulations/RESULTS_KEY.json'
TIMINGS = 'launch/simulations/TIMINGS.json'
DOMAIN = 'dytallix-launch-results-v1:'
WALLET = 'launch-results'
PUBLIC_KEY_HEX = staging_command.agent.PUBLIC_KEY_HEX
SIGNATURE = staging_command.SIGNATURE
COMMIT = re.compile(r'[0-9a-f]{40}')
SHA256 = re.compile(r'[0-9a-f]{64}')
# A fault scenario's campaign KPI and its approved target in TIMINGS.json.
FAULT_KPIS = {'partition': ('campaign.faults.network_heal', 'fault_recovery_target_seconds'),
              'validator_restart': ('campaign.faults.validator_restart', 'validator_restart_target_seconds')}
QUICK_REPETITIONS = 30


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def digest(raw): return hashlib.sha256(raw).hexdigest()


def render(summary): return json.dumps(summary, indent=2) + '\n'


def message(raw): return DOMAIN + digest(raw)


def day(text): return faults.instant(text).astimezone(timezone.utc).date().isoformat()


def candidate(release_record, commit):
    """The candidate a summary measured: its source commit and release set."""
    require(isinstance(commit, str) and COMMIT.fullmatch(commit) is not None, 'the commit is not a 40-hex git commit')
    found = release_record.get('release_set_sha256')
    require(release_record.get('schema') == 'dytallix.release-build-record.v1' and isinstance(found, str)
            and SHA256.fullmatch(found) is not None, 'not a release build record')
    return {'commit': commit, 'release_set_sha256': found}


def summarize_campaign(fault_files, genesis_bytes, engine_genesis_bytes, timings, release_record, commit):
    """The campaign summary from every fault record to date (cumulative)."""
    samples = {kpi: [] for kpi, _ in FAULT_KPIS.values()}
    sources, days = [], []
    for raw in fault_files:
        record = json.loads(raw)
        try:
            status = faults.verify(record, genesis_bytes, engine_genesis_bytes)
        except (faults.Failed, KeyError, TypeError) as error:
            raise Failed(f'a fault record does not verify: {error}')
        for row in record['scenarios']:
            if row['name'] not in FAULT_KPIS:
                continue
            kpi, key = FAULT_KPIS[row['name']]
            require(record['recovery_target_seconds'] == timings[key],
                    f'{row["name"]} was judged against {record["recovery_target_seconds"]} s, '
                    f'not the approved {timings[key]} s')
            if row['status'] == faults.INCONCLUSIVE:
                continue
            samples[kpi].append((row['status'], row.get('recovery_seconds')))
            days.append(day(row.get('recovered_at') or row['cleared_at']))
        sources.append({'tool': 'faults', 'sha256': digest(raw), 'chain_id': record['chain_id'],
                        'native_genesis_sha256': record['genesis']['native_genesis_sha256'], 'status': status})
    require(days, 'the evidence holds no recovery for a campaign KPI')
    kpis = []
    for kpi, key in FAULT_KPIS.values():
        rows = samples[kpi]
        if not rows:
            continue
        seconds = [s for _, s in rows if isinstance(s, (int, float))]
        failed = any(status == faults.FAIL for status, _ in rows)
        status = 'fail' if failed else 'pass' if len(rows) >= QUICK_REPETITIONS else 'in_progress'
        kpis.append({'id': kpi, 'status': status, 'value': round(max(seconds), 1) if seconds else None, 'unit': 's',
                     'samples': len(rows), 'target': timings[key], 'repetitions': QUICK_REPETITIONS})
    summary = {'schema': SCHEMA, 'scope': 'campaign', 'date': max(days),
               'candidate': candidate(release_record, commit), 'kpis': kpis,
               'sources': sorted(sources, key=lambda s: s['sha256'])}
    problems = summary_problems(summary)
    require(not problems, '; '.join(problems))
    return summary


def summary_problems(summary, root=ROOT):
    """launch_dashboard's own check of a summary, with the page's definitions."""
    definitions = json.loads((Path(root) / launch_dashboard.DEFINITIONS).read_bytes())
    return launch_dashboard.summary_problems(summary, definitions)


def keys(root=ROOT):
    record = json.loads((Path(root) / KEYS).read_bytes())
    require(record.get('schema') == KEY_SCHEMA and record.get('keys'), f'{KEYS} is not a results key record')
    for key in record['keys']:
        require(len(key['public_key_hex']) == PUBLIC_KEY_HEX and digest(bytes.fromhex(key['public_key_hex']))
                == key['key_id'], f'{KEYS}: a key does not match its key ID')
    return record['keys']


def keystore_key(home):
    keystore = json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())
    require(keystore.get('active') == WALLET, f'the keystore\'s active wallet is not {WALLET}')
    entry = next((e for e in keystore['entries'] if e['name'] == WALLET), None)
    require(entry is not None, f'the keystore has no {WALLET} wallet')
    return bytes(entry['public_key']).hex()


def sign(cli, home, path, root=ROOT, run=subprocess.run):
    """Signs a summary with the results key and checks the signature."""
    raw = Path(path).read_bytes()
    problems = summary_problems(json.loads(raw), root)
    require(not problems, f'{path}: {"; ".join(problems)}')
    newest = keys(root)[-1]['public_key_hex']
    require(keystore_key(home) == newest, f'the keystore\'s results key is not the newest in {KEYS}')
    text = message(raw)
    result = run([str(cli), 'crypto', 'sign', text], env=staging_command.cli_env(home), capture_output=True, text=True)
    require(result.returncode == 0, f'dytallix crypto sign failed: {(result.stderr or result.stdout).strip()}')
    found = SIGNATURE.findall(result.stdout)
    require(len(found) == 1, 'dytallix crypto sign printed no ML-DSA-65 signature')
    checked = run([str(cli), 'crypto', 'verify', text, found[0], newest], capture_output=True, text=True)
    require(checked.returncode == 0, 'the new signature does not verify')
    Path(f'{path}.sig').write_text(found[0] + '\n')
    return found[0]


def verify(cli, path, root=ROOT, run=subprocess.run):
    """A summary's page rules and its signature under a listed results key."""
    raw = Path(path).read_bytes()
    problems = summary_problems(json.loads(raw), root)
    require(not problems, f'{path}: {"; ".join(problems)}')
    signature = Path(f'{path}.sig').read_text().strip()
    require(launch_dashboard.SIGNATURE.fullmatch(signature) is not None,
            f'{path}.sig is not an ML-DSA-65 signature in hex')
    for key in keys(root):
        if run([str(cli), 'crypto', 'verify', message(raw), signature, key['public_key_hex']],
               capture_output=True, text=True).returncode == 0:
            return key['key_id']
    raise Failed(f'{path}: the signature verifies under no key in {KEYS}')


def write_summary(summary, root=ROOT):
    path = Path(root) / launch_dashboard.RESULTS / summary['scope'] / f'{summary["date"]}.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    raw = render(summary).encode()
    if path.exists() and path.read_bytes() != raw:
        # A changed summary needs a new signature.
        Path(f'{path}.sig').unlink(missing_ok=True)
    path.write_bytes(raw)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    s = commands.add_parser('summarize')
    s.add_argument('scope', choices=['campaign'])
    s.add_argument('--faults', type=Path, nargs='+', required=True)
    for name in ('genesis', 'engine_genesis', 'release'):
        s.add_argument(f'--{name.replace("_", "-")}', type=Path, required=True)
    s.add_argument('--commit', required=True)
    s.add_argument('--root', type=Path, default=ROOT)
    g = commands.add_parser('sign')
    g.add_argument('--cli', required=True)
    g.add_argument('--home', type=Path, required=True)
    g.add_argument('summary', type=Path)
    v = commands.add_parser('verify')
    v.add_argument('--cli', required=True)
    v.add_argument('summary', type=Path, nargs='+')
    args = parser.parse_args()
    try:
        if args.command == 'summarize':
            timings = json.loads((ROOT / TIMINGS).read_bytes())
            summary = summarize_campaign([p.read_bytes() for p in args.faults], args.genesis.read_bytes(),
                                         args.engine_genesis.read_bytes(), timings,
                                         json.loads(args.release.read_bytes()), args.commit)
            path = write_summary(summary, args.root)
            print(f'{path} ({", ".join(f"{k["id"]} {k["status"]}" for k in summary["kpis"])}); sign it next')
        elif args.command == 'sign':
            sign(args.cli, args.home, args.summary)
            print(f'{args.summary}.sig')
        else:
            for path in args.summary:
                print(f'{path}: signed with results key {verify(args.cli, path)[:16]}')
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'results: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
