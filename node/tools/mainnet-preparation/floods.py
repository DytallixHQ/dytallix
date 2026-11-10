#!/usr/bin/env python3
"""The attack campaign's flood family (simulation plan, T4; P01, 9 November
2026, launch/approvals/P01_T04_FLOODS_2026-11-09.json): each type 10 times
through the sentry's client channel, with zero successes.

  floods.py run --cli DYTALLIX --home HOME --fleet DIR --out DIR
                --application-config FILE --funder WALLET --validator ID
                --genesis NATIVE_GENESIS --engine-genesis ENGINE_GENESIS
                [--attempts N] [--types T,...] [--spaces NS,...]
  floods.py hammer --cli DYTALLIX --seconds S     (one flooder; HOME pinned)
  floods.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                   --engine-genesis ENGINE_GENESIS

The fleet (load.py fleet) holds at least the queue bound plus FLOOD_EXTRA
load wallets. Each type and what counts as the node refusing it:
  - transaction_flood: one valid transfer from every wallet, signed first and
    then sent at once after a fresh block, more than the admission queue's
    entry bound (queue_max_entries, read from the application config and
    recorded). The ones past the bound must be refused with the queue's
    capacity reason, and every one admitted must commit (a full queue
    refuses, never evicts: mempool rule 5). More admitted than the bound
    while every batch met the same head is a success (a defect), as is an
    admitted transaction that never commits. A flood that met no full queue
    because a block came in between proves nothing and is made again.
  - request_flood: flooders at FLOODERS addresses (each the channel's
    per-address bound of 2 connections, 14 of its 16) send status requests
    as fast as they are answered for FLOOD_SECONDS, while an honest client
    at another address keeps its 2 and reads the head about once a second.
    Every honest read must be served and the head must advance; a read the
    channel did not serve, or a chain that stood still, is a success. Each
    attempt also records, outside the judgement, the honest client's misses
    (its error and how long the read took) and its slowest read, the
    flooders' first errors and the runner's load averages, so a miss can
    be traced (P01, 9 November 2026). A
    flood that sent nothing is made again.
The queue bound is the chain's (genesis) value: the CI staging chain is
built with 64 (TIMINGS.json, ci profile), the production and rehearsal
chains with 2,000, and the record names the bound it was judged against.

After the transaction floods a valid control transfer must commit, and the
head must rise from control to control, so a channel that refuses
everything, or a chain that stopped, cannot pass.

verify recomputes every outcome, count, status and digest from the rows.
The rows and the evidence name no address; the evidence binds the genesis
files.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import attacks  # noqa: E402
import load  # noqa: E402

SCHEMA = attacks.SCHEMA
FAMILY = 'floods'
PASS, FAIL = attacks.PASS, attacks.FAIL
TARGET = 10
OUTCOMES = attacks.OUTCOMES
REASONS = {
    'transaction_flood': 'transactions past the queue bound are refused, and none admitted is evicted',
    'request_flood': 'an honest client keeps being served while the other addresses flood the channel',
}
QUEUE_REFUSAL = 'Queue resource capacity exceeded'
# Wallets past the bound, so a flood that meets one head always overfills it.
FLOOD_EXTRA = 16
# 7 flooders of 2 connections: 14 of the channel's 16, so the honest address
# keeps its 2 (P01, 9 November 2026).
FLOODERS = 7
FLOOD_SECONDS = 20
HONEST_PERIOD = 1.0
CHANNEL = {'channel_total': 16, 'channel_per_address': 2}
# A transaction flood that met a full queue never gets more tries than this.
MAX_TRIES = 3

digest, canonical, rows_bytes = attacks.digest, attacks.canonical, attacks.rows_bytes


class Failed(Exception):
    pass


def classify(row):
    """An attempt's outcome from its observations alone."""
    kind, seen = row['type'], row['observed']
    if seen.get('error'):
        return 'transport'
    if kind == 'transaction_flood':
        if seen['other_refusals']:
            return 'refused_other'
        if seen['lost']:
            return 'admitted'
        if len(seen['heads']) == 1 and seen['admitted'] > seen['bound']:
            return 'admitted'
        if not seen['queue_refused']:
            return 'transport'
        return 'refused'
    if kind == 'request_flood':
        if not seen['flood_served']:
            return 'transport'
        if seen['honest_served'] < seen['honest_reads'] or not seen['honest_reads']:
            return 'admitted'
        first, last = seen['honest_heights']
        return 'refused' if last > first else 'admitted'
    raise Failed(f'{kind!r} has no observations to judge')


def describe(row):
    return f'{row["type"]}: {json.dumps(row["observed"], sort_keys=True)}'


def tally(rows, attempts):
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
                       'first_log': next((r['log'] for r in mine), None)}
    return types


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, bound, started, finished):
    for row in rows:
        if row['type'] != 'control':
            if classify(row) != row['outcome'] or describe(row) != row['log']:
                raise Failed(f'a {row["type"]} row\'s outcome does not follow from its observations')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    heights = [r.get('height') for r in controls]
    checks = {
        'types': all(t['status'] in (PASS, 'not_run') for t in types.values()),
        'controls': bool(controls) and all(r.get('committed') is True for r in controls),
        'advancing': all(isinstance(h, int) for h in heights) and all(a < b for a, b in zip(heights, heights[1:])),
    }
    body = {'schema': SCHEMA, 'family': FAMILY, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'started': started, 'finished': finished, 'attempts_wanted': attempts,
            'limits': dict(CHANNEL, queue_max_entries=bound, flooders=FLOODERS, flood_seconds=FLOOD_SECONDS),
            'types': types,
            'controls': {'made': len(controls), 'committed': sum(r.get('committed') is True for r in controls)},
            'successes': sum(t['counts']['admitted'] for t in types.values()),
            'rows_sha256': digest(raw), 'checks': checks,
            'status': PASS if all(checks.values()) else FAIL}
    body['sha256'] = digest(canonical(body))
    return body


def verify(record, raw, genesis_bytes, engine_genesis_bytes):
    """Re-evaluates a campaign record from its rows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['family'] != FAMILY:
        raise Failed(f'the record is for {record["family"]}, not {FAMILY}')
    bound = record['limits'].get('queue_max_entries')
    if record['limits'] != dict(CHANNEL, queue_max_entries=bound, flooders=FLOODERS, flood_seconds=FLOOD_SECONDS) \
            or not (isinstance(bound, int) and bound > 0):
        raise Failed(f'the record was judged against {record["limits"]}, not the approved bounds')
    if digest(raw) != record['rows_sha256']:
        raise Failed('the rows are not the ones the evidence names')
    if record['genesis'] != {'native_genesis_sha256': digest(genesis_bytes),
                             'engine_genesis_sha256': digest(engine_genesis_bytes)}:
        raise Failed('the evidence names another genesis')
    rows = [json.loads(line) for line in raw.splitlines() if line.strip()]
    for row in rows:
        if row['type'] == 'transaction_flood' and row['observed'].get('bound') not in (None, bound):
            raise Failed('a transaction flood was judged against another queue bound')
    again = build_record(record['chain_id'], genesis_bytes, engine_genesis_bytes, rows, record['attempts_wanted'],
                         bound, record['started'], record['finished'])
    if again != record:
        raise Failed('the counts and statuses do not follow from the rows')
    return record['status']


def finish(kind, attempt, observed):
    row = {'type': kind, 'attempt': attempt, 'observed': observed}
    row['outcome'], row['log'] = classify(row), describe(row)
    return row


# The transaction flood

def transaction_observed(rows, bound):
    admitted = [r for r in rows if r['status'] in ('admitted', 'committed', 'lost')]
    refused = [r for r in rows if r['status'] == 'refused']
    return {'bound': bound, 'sent': len(rows),
            'heads': sorted({r['submit_height'] for r in rows if r['submit_height'] is not None}),
            'admitted': len(admitted), 'committed': sum(r['status'] == 'committed' for r in rows),
            'lost': sum(r['status'] == 'lost' for r in rows),
            'queue_refused': sum(QUEUE_REFUSAL in (r['log'] or '') for r in refused),
            'other_refusals': sorted({(r['log'] or '')[:160] for r in refused if QUEUE_REFUSAL not in (r['log'] or '')})}


def wait_new_head(rpc, timeout=60, sleep=time.sleep, clock=time.monotonic):
    start = load.head_height(rpc.one('head', 'abci_info', {}))
    deadline = clock() + timeout
    while clock() < deadline:
        head = load.head_height(rpc.one('head', 'abci_info', {}))
        if head > start:
            return head
        sleep(0.2)
    raise Failed(f'the head stayed at {start} for {timeout} s')


def transaction_flood(run, rpc, wallets, funder, bound, show):
    """One flood: every wallet's transfer signed, then sent at once just
    after a block, then followed to its receipt or its expiry."""
    run.refresh_profile()
    run.sync(wallets)
    actions = load.actions_for('send_drt', recipient_hex=funder.account_id)

    def sign(wallet):
        return run.sign(load.new_row('flood', 'transaction_flood', wallet), wallet, actions)

    with ThreadPoolExecutor(max(1, run.s.signers)) as pool:
        signed = list(pool.map(sign, wallets))
    batches = [signed[n:n + load.MAX_BATCH - 1] for n in range(0, len(signed), load.MAX_BATCH - 1)]
    wait_new_head(rpc)
    errors = []

    def send(share):
        for batch in share:
            try:
                load.broadcast(rpc, batch, 'flood')
            except load.Transport as error:
                errors.append(str(error)[:200])

    shares = [batches[n::rpc.connections] for n in range(rpc.connections)]
    threads = [threading.Thread(target=send, args=(share,)) for share in shares]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    rows = [row for row, _ in signed]
    for row in rows:
        if row['status'] in ('admitted', 'uncertain'):
            run.tracker.add(row)
    run.tracker.drain(run.s.block_time * (run.s.expiry_blocks + 12), interval=run.s.poll_interval)
    observed = transaction_observed(rows, bound)
    if errors:
        observed['error'] = errors[0]
    show(f'== transaction_flood: {observed["admitted"]} admitted at heads {observed["heads"]}, '
         f'{observed["queue_refused"]} refused as the queue was full, {observed["lost"]} lost')
    return observed


# The request flood

def hammer(cli, seconds, threads=2):
    """One flooder: status requests through its own gateway, as fast as they
    are answered, from `threads` connections. Prints its counts."""
    env = dict(os.environ)
    gateway = load.Gateway(cli, env, os.devnull)
    served, errors, first = [0], [0], []
    lock = threading.Lock()
    try:
        rpc = load.Rpc(gateway.url, threads, timeout=10)
        deadline = time.monotonic() + seconds

        def loop():
            while time.monotonic() < deadline:
                try:
                    load.head_height(rpc.one('flood', 'abci_info', {}))
                    with lock:
                        served[0] += 1
                except load.Failed as error:
                    with lock:
                        errors[0] += 1
                        if len(first) < 3:
                            first.append(str(error)[:160])
        workers = [threading.Thread(target=loop) for _ in range(threads)]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join()
    finally:
        gateway.stop()
    print(json.dumps({'served': served[0], 'errors': errors[0], 'first_errors': first}))


def request_flood(rpc, cli, env, spaces, show, seconds=FLOOD_SECONDS):
    """FLOODERS flooders, one in each namespace, while the honest reader on
    this address reads the head about once a second."""
    command = [sys.executable, '-I', '-B', str(HERE / 'floods.py'), 'hammer', '--cli', str(cli),
               '--seconds', str(seconds)]
    flooders = [subprocess.Popen(['ip', 'netns', 'exec', space, *command], stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, text=True, env=env) for space in spaces[:FLOODERS]]
    reads, served, heights, misses, slowest = 0, 0, [], [], 0.0
    # The flooders' gateways start first.
    time.sleep(3)
    end = time.monotonic() + seconds - 4
    while time.monotonic() < end:
        reads += 1
        began = time.monotonic()
        try:
            heights.append(load.head_height(rpc.one('honest', 'abci_info', {})))
            served += 1
        except load.Failed as error:
            # Diagnostics (P01, 9 November 2026: one honest read in 30
            # attempts was not served; the cause is to be found, the
            # criterion stays): what the read got and how long it took.
            misses.append({'after_s': round(time.monotonic() - began, 2), 'error': str(error)[:160]})
        slowest = max(slowest, time.monotonic() - began)
        time.sleep(HONEST_PERIOD)
    counts, failures = [], []
    for flooder in flooders:
        out, err = flooder.communicate(timeout=seconds + 60)
        try:
            counts.append(json.loads(out.strip().splitlines()[-1]))
        except (ValueError, IndexError):
            failures.append(err.strip()[-200:])
    observed = {'flooders': len(flooders), 'flood_served': sum(c['served'] for c in counts),
                'flood_errors': sum(c['errors'] for c in counts), 'honest_reads': reads, 'honest_served': served,
                'honest_heights': [heights[0], heights[-1]] if heights else [None, None],
                'honest_slowest_s': round(slowest, 2), 'honest_misses': misses[:3],
                'flood_first_errors': [e for c in counts for e in c.get('first_errors', [])][:3],
                'runner_load': [round(v, 2) for v in os.getloadavg()]}
    if failures or len(counts) < FLOODERS:
        observed['error'] = f'a flooder did not report: {failures[:1]}'
    show(f'== request_flood: flooders served {observed["flood_served"]} ({observed["flood_errors"]} not), '
         f'honest {served}/{reads}, heights {observed["honest_heights"]}')
    return observed


def control(run, wallet, funder):
    row = run.online(wallet, ('send', '--to', funder.address, '--amount', '1', '--token', 'drt'),
                     phase='flood', kind='control')
    committed = False
    if row['status'] == 'admitted':
        run.follow(row)
        committed = True
    return {'type': 'control', 'committed': committed, 'height': row.get('block_height'),
            'code': row.get('code')}


def stamp():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def run_campaign(cli, home, fleet_dir, out, application_config, funder_label, validator, native, engine,
                 attempts=TARGET, kinds=tuple(REASONS), spaces=(), show=load.say, passphrase=None, settings=None):
    out, fleet_dir = Path(out), Path(fleet_dir)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(home))
    if passphrase:
        env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    config = json.loads(Path(application_config).read_bytes())
    bound = int(config['ordinary']['queue_max_entries'])
    fleet = json.loads((fleet_dir / 'fleet.json').read_bytes())
    fleet['_dir'] = str(fleet_dir)
    chain_id = fleet['chain_id']
    entries = {e['name']: e for e in json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())['entries']}
    funder = load.keystore_account(entries, funder_label, chain_id)
    if 'request_flood' in kinds and len(spaces) < FLOODERS:
        raise Failed(f'the request flood needs {FLOODERS} addresses of its own (--spaces)')
    started, rows = stamp(), []
    gateway = load.Gateway(cli, env, out / 'gateway.log')
    try:
        rpc = load.Rpc(gateway.url, 2)
        run = load.Load(rpc, load.Signer(cli, env, out / 'scratch'), env, cli, fleet, out,
                        settings or load.Settings(), funder, validator, config, show=show)
        wallets = run.role('load')
        if 'transaction_flood' in kinds and len(wallets) < bound + FLOOD_EXTRA:
            raise Failed(f'the transaction flood needs {bound + FLOOD_EXTRA} load wallets for a queue of {bound}')
        run.fund()
        run.first_spends()
        if 'transaction_flood' in kinds:
            flood_wallets = wallets[:bound + FLOOD_EXTRA]
            for attempt in range(attempts):
                for _ in range(MAX_TRIES):
                    row = finish('transaction_flood', attempt, transaction_flood(run, rpc, flood_wallets, run.funder,
                                                                                bound, show))
                    rows.append(row)
                    if row['outcome'] != 'transport':
                        break
        if 'request_flood' in kinds:
            for attempt in range(attempts):
                for _ in range(MAX_TRIES):
                    row = finish('request_flood', attempt, request_flood(rpc, cli, env, spaces, show))
                    rows.append(row)
                    if row['outcome'] != 'transport':
                        break
        run.sync([wallets[0]])
        rows.append(control(run, wallets[0], run.funder))
        rows.append(control(run, wallets[0], run.funder))
    finally:
        gateway.stop()
    record = build_record(chain_id, native, engine, rows, attempts, bound, started, stamp())
    (out / 'floods.jsonl').write_bytes(rows_bytes(rows))
    (out / 'floods.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


def report(record, show=print):
    for kind, found in record['types'].items():
        show(f'== {kind}: {found["status"]} {found["counts"]}')
    show(f'== floods: {record["status"]} (checks {record["checks"]}, queue bound {record["limits"]["queue_max_entries"]})')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    r = commands.add_parser('run')
    for name in ('cli', 'home', 'fleet', 'out', 'application_config', 'genesis', 'engine_genesis'):
        r.add_argument(f'--{name.replace("_", "-")}', type=Path, required=True)
    r.add_argument('--funder', required=True)
    r.add_argument('--validator', required=True)
    r.add_argument('--attempts', type=int, default=TARGET)
    r.add_argument('--types', default=','.join(REASONS))
    r.add_argument('--spaces', default='')
    h = commands.add_parser('hammer')
    h.add_argument('--cli', type=Path, required=True)
    h.add_argument('--seconds', type=float, required=True)
    v = commands.add_parser('verify')
    for name in ('evidence', 'rows', 'genesis', 'engine_genesis'):
        v.add_argument(f'--{name.replace("_", "-")}', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'hammer':
            hammer(args.cli, args.seconds)
            return 0
        if args.command == 'run':
            kinds = tuple(k for k in args.types.split(',') if k)
            unknown = set(kinds) - set(REASONS)
            if unknown:
                raise Failed(f'not flood types: {sorted(unknown)}')
            record = run_campaign(args.cli, args.home, args.fleet, args.out, args.application_config, args.funder,
                                  args.validator, args.genesis.read_bytes(), args.engine_genesis.read_bytes(),
                                  args.attempts, kinds, [s for s in args.spaces.split(',') if s])
            report(record)
            return 0 if record['status'] == PASS else 1
        status = verify(json.loads(args.evidence.read_bytes()), args.rows.read_bytes(), args.genesis.read_bytes(),
                        args.engine_genesis.read_bytes())
    except (Failed, load.Failed, OSError, KeyError, ValueError) as error:
        print(f'floods: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
