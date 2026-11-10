#!/usr/bin/env python3
"""The flood family (T4; P01, 9 November 2026,
launch/approvals/P01_T04_FLOODS_2026-11-09.json) from the load machines
(launch/approvals/P01_T06_LOAD_MACHINES_HETZNER_2026-11-03.json), for the
real-host run against the production queue bound of 2,000. floods.py run is
the CI mode (one runner, network namespaces); this is the same campaign with
the flooding spread over machines that each have their own address, judged
into the same record, so `floods.py verify` checks it unchanged.

  flood_workers.py drive --cli DYTALLIX --home HOME --fleet DIR --out DIR
                         --application-config FILE --funder WALLET --validator ID
                         --genesis NATIVE_GENESIS --engine-genesis ENGINE_GENESIS
                         --workers N (--machines RUN_DIR [--worker-cli DYTALLIX]
                                      | --local [--namespaces NS,... --addresses A,...])
                         [--attempts N] [--types T,...] [--lead S]
  flood_workers.py work --cli DYTALLIX --home HOME --shard DIR --out DIR
                        --application-config FILE --validator ID

It reuses load_workers.py's machinery (the shares, the shipped files, the
SSH and local transports, the file handshake) and floods.py's own pieces:
  1. drive funds the fleet's load wallets from the funder, through its
     distributors, on the founder's machine (the funder never leaves it), and
     splits the queue bound plus FLOOD_EXTRA flood wallets into one disjoint
     share per worker (load_workers.split);
  2. each worker primes its share (the first spends) through its own gateway
     and reports primed;
  3. a transaction flood is two commands to every worker: sign (each signs
     its share's transfers to the funder against a fresh profile view), then
     send at a target head: drive reads the head H once and names
     H + 1 + ceil(lead / block time); each worker polls its own head and sends
     its whole share over its connections as soon as the head reaches the
     target, so every machine sends just after the same block, then follows
     its rows to their receipts or expiry and returns them. drive merges
     every worker's rows (digest-checked) and judges them with floods.py's
     transaction_observed, as one flood. drive makes no channel request from
     the send command until the last worker's rows, as load_workers.py's
     coordinator does not during its window; the workers' connections must
     fit the channel (load_workers.budget);
  4. a request flood: each worker first stops its own gateway, so that its
     address holds only its flooder's connections; FLOODERS of the workers
     run floods.py hammer (2 connections each) from a common start time,
     while drive, on the founder's address, is the honest client
     (floods.honest_reads); the observations are floods.request_observed's.
     It needs FLOODERS machines, each with its own address: the approved
     plan is 7 addresses x 2 connections, 14 of the channel's 16;
  5. after both, the workers stop and drive makes the two control transfers.

The record and the rows are floods.py's (build_record), so they name no
address. Each worker's address, share size, priming time and send heads go in
workers.json beside them, which stays off the repository as load_workers.py's
evidence does.
"""
import argparse
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import floods  # noqa: E402
import load  # noqa: E402
import load_workers  # noqa: E402

WORKER_SCHEMA = 'dytallix.flood-worker.v1'
# What a worker runs: flood_workers.py and everything it imports from here.
TOOLS = load_workers.TOOLS + ('flood_workers.py', 'floods.py', 'attacks.py', 'adversary.py', 'reconcile.py',
                              'host/monitor.py')
# Seconds drive gives its commands to reach every machine (load_workers.py's
# lead before its window).
LEAD = 15.0
# How often a worker reads its head while it waits for the target.
HEAD_POLL = 0.2

Failed = load.Failed


# The worker

def command_name(seq):
    return f'command-{seq:03}.json'


def reply_name(seq):
    return f'reply-{seq:03}.json'


def rows_name(seq):
    return f'rows-{seq:03}.jsonl'


def wait_height(rpc, height, timeout, sleep=time.sleep, clock=time.monotonic):
    """Returns once the head reaches `height` (at once if it already has)."""
    deadline = clock() + timeout
    while True:
        head = load.head_height(rpc.one('head', 'abci_info', {}))
        if head >= height:
            return head
        if clock() > deadline:
            raise Failed(f'the head stayed at {head}, below {height}, for {timeout} s')
        sleep(HEAD_POLL)


def run_hammer(cli, env, seconds):
    """floods.py hammer as its own process, as in CI: its counts, or why it
    did not report."""
    done = subprocess.run([sys.executable, '-I', '-B', str(HERE / 'floods.py'), 'hammer', '--cli', str(cli),
                           '--seconds', str(seconds)], capture_output=True, text=True, env=env,
                          timeout=seconds + 60)
    try:
        return json.loads(done.stdout.strip().splitlines()[-1]), None
    except (ValueError, IndexError):
        return None, done.stderr.strip()[-200:]


def serve(run, shard, out, close, hammer, clock=time.time, sleep=time.sleep, poll=0.5, idle_timeout=7200):
    """A worker's life: prime the share, then each command drive writes, in
    order, until stop. close() stops the worker's own gateway; hammer(seconds)
    is one flooder."""
    out = Path(out)
    for record in shard['wallets']:
        wallet = run.wallets[record['label']]
        wallet.drt, wallet.dgt = int(record['funded_udrt']), int(record['funded_udgt'])
    mark = clock()
    run.first_spends()
    load_workers.write_json(out / 'primed.json', {'schema': WORKER_SCHEMA, 'shard': shard['shard'],
                                                  'wallets': len(shard['wallets']),
                                                  'primed_s': round(clock() - mark, 1)})
    wallets = run.role('load')
    signed, seq = None, 0
    while True:
        path, deadline = out / command_name(seq), clock() + idle_timeout
        while not path.exists():
            if clock() > deadline:
                raise Failed(f'no command {seq} within {idle_timeout} s')
            sleep(poll)
        command = json.loads(path.read_bytes())
        do, reply = command['do'], {}
        if do == 'sign':
            mark = clock()
            signed = floods.sign_share(run, wallets, command['recipient'])
            reply = {'signed': len(signed), 'sign_s': round(clock() - mark, 1)}
        elif do == 'send':
            if signed is None:
                raise Failed('a send without a signed share')
            heads = []

            def wait():
                heads.append(wait_height(run.rpc, command['height'], command['timeout'], sleep))
            rows, errors = floods.send_signed(run, run.rpc, signed, wait)
            signed = None
            raw = load.rows_jsonl(rows)
            (out / rows_name(seq)).write_bytes(raw)
            reply = {'rows': len(rows), 'rows_sha256': load.sha256(raw), 'errors': errors[:3],
                     'sent_at_head': heads[0] if heads else None}
        elif do in ('hammer', 'idle'):
            close()
            while clock() < command['start_at']:
                sleep(min(poll, max(0.0, command['start_at'] - clock())))
            if do == 'hammer':
                counts, failure = hammer(command['seconds'])
                reply = {'counts': counts, 'failure': failure}
        elif do != 'stop':
            raise Failed(f'an unknown command {do!r}')
        load_workers.write_json(out / reply_name(seq), dict(reply, schema=WORKER_SCHEMA, seq=seq, do=do))
        if do == 'stop':
            return
        seq += 1


def work(cli, home, shard_dir, out, application_config, validator, show=load.say):
    """A worker on a load machine; an error is left in error.json."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    try:
        shard = json.loads((Path(shard_dir) / 'fleet.json').read_bytes())
        if shard.get('schema') != load_workers.SHARD_SCHEMA:
            raise Failed(f'{shard_dir}/fleet.json is not {load_workers.SHARD_SCHEMA}')
        settings = load.Settings(**dict(shard['settings'], governance=False))
        env = dict(os.environ, HOME=str(home))
        config = json.loads(Path(application_config).read_bytes())
        fleet = {'_dir': str(shard_dir), 'chain_id': shard['chain_id'], 'wallets': shard['wallets']}
        gateway = load.Gateway(cli, env, out / 'gateway.log')
        try:
            show(f'== flood worker {shard["shard"]}: {len(fleet["wallets"])} wallets through {gateway.endpoint}')
            rpc = load.Rpc(gateway.url, settings.connections)
            run = load.Load(rpc, load.Signer(cli, env, out / 'scratch'), env, cli, fleet, out, settings, None,
                            validator, config, show=show)
            serve(run, shard, out, gateway.stop, lambda seconds: run_hammer(cli, env, seconds))
            shutil.rmtree(out / 'scratch', ignore_errors=True)
        finally:
            # Stopping twice is harmless.
            if gateway.process.poll() is None:
                gateway.stop()
    except Exception as error:
        load_workers.write_json(out / 'error.json', {'error': f'{type(error).__name__}: {error}'[:500]})
        raise


# Transports: load_workers.py's, running this file's worker

class LocalTransport(load_workers.LocalTransport):
    def worker_argv(self, n):
        argv = super().worker_argv(n)
        argv[3] = str(self.folder(n) / 'flood_workers.py')
        return argv


class SshTransport(load_workers.SshTransport):
    WORKER = '"[f]lood_workers.py work"'

    def start(self, n):
        r = shlex.quote(self.remote)
        argv = ['python3', '-I', '-B', 'flood_workers.py', 'work', '--cli', './dytallix', '--home', './home',
                '--shard', './shard', '--out', './out', '--application-config', './application-config.json',
                '--validator', self.validator]
        self.call(n, f'cd {r} && (setsid nohup {shlex.join(argv)} > work.log 2>&1 < /dev/null &)')


# The coordinator

class Workers:
    """Every worker gets every command, in the same order, so that command
    and reply n mean the same thing on each."""

    def __init__(self, transport, count, poll, clock=time.time, sleep=time.sleep):
        self.transport, self.count, self.poll = transport, count, poll
        self.clock, self.sleep, self.seq = clock, sleep, 0

    def ask(self, commands, timeout):
        """commands[n] to worker n; every reply."""
        for n, command in enumerate(commands):
            self.transport.write(n, command_name(self.seq), command)
        replies = load_workers.wait_for(self.transport, self.count, reply_name(self.seq), timeout, self.clock,
                                        self.sleep, self.poll)
        self.seq += 1
        return replies

    def rows(self, n, seq, reply):
        raw = self.transport.read(n, rows_name(seq))
        if raw is None or load.sha256(raw) != reply['rows_sha256']:
            raise Failed(f'worker {n}\'s rows are not the ones its reply names')
        return [json.loads(line) for line in raw.splitlines() if line.strip()]


def target_height(head, lead, block_time):
    """The block every worker sends after: far enough ahead that the send
    command reaches every machine first."""
    return head + 1 + math.ceil(lead / block_time)


def machine_transaction_flood(run, rpc, workers, bound, lead, show):
    """One transaction flood over every worker's share, judged as one."""
    signs = workers.ask([{'do': 'sign', 'recipient': run.funder.account_id}] * workers.count, 3600)
    head = load.head_height(rpc.one('head', 'abci_info', {}))
    height = target_height(head, lead, run.s.block_time)
    # No channel request from here until every worker's rows are back.
    seq = workers.seq
    timeout = lead + run.s.block_time * (run.s.expiry_blocks + 12) + 300
    replies = workers.ask([{'do': 'send', 'height': height, 'timeout': lead + 120}] * workers.count, timeout)
    rows = [row for n, reply in enumerate(replies) for row in workers.rows(n, seq, reply)]
    observed = floods.transaction_observed(rows, bound)
    errors = [e for reply in replies for e in reply['errors']]
    if errors:
        observed['error'] = errors[0]
    show(f'== transaction_flood from {workers.count} machines after head {height} '
         f'(signed {sum(s["signed"] for s in signs)}): {observed["admitted"]} admitted at heads {observed["heads"]}, '
         f'{observed["queue_refused"]} refused as the queue was full, {observed["lost"]} lost')
    return observed, [r.get('sent_at_head') for r in replies]


def machine_request_flood(rpc, workers, flooders, lead, show, seconds=floods.FLOOD_SECONDS):
    """FLOODERS workers flood from a common start time while this address is
    the honest client; the other workers idle with their gateways stopped."""
    start_at = workers.clock() + lead
    commands = [{'do': 'hammer' if n < flooders else 'idle', 'start_at': start_at, 'seconds': seconds}
                for n in range(workers.count)]
    for n, command in enumerate(commands):
        workers.transport.write(n, command_name(workers.seq), command)
    while workers.clock() < start_at:
        workers.sleep(min(1.0, max(0.0, start_at - workers.clock())))
    honest = floods.honest_reads(rpc, seconds)
    replies = load_workers.wait_for(workers.transport, workers.count, reply_name(workers.seq), seconds + 180,
                                    workers.clock, workers.sleep, workers.poll)
    workers.seq += 1
    hammers = [r for r in replies if r['do'] == 'hammer']
    counts = [r['counts'] for r in hammers if r['counts'] is not None]
    failures = [r['failure'] or '' for r in hammers if r['counts'] is None]
    return floods.request_observed(len(hammers), counts, failures, honest, show)


def check_plan(count, kinds, separate, flooders=floods.FLOODERS):
    """The machines must fit the channel and, for the request flood, give
    the approved flooder addresses."""
    load_workers.budget(count, load_workers.PER_ADDRESS, separate)
    if 'request_flood' in kinds:
        if not separate:
            raise Failed('the request flood needs each worker at its own address')
        if count < flooders:
            raise Failed(f'the request flood needs {flooders} machines, each its own address holding '
                         f'{load_workers.PER_ADDRESS} connections ({flooders * load_workers.PER_ADDRESS} of the '
                         f'channel\'s {load_workers.CHANNEL_CONNECTIONS}); {count} were given')


def campaign(run, rpc, fleet, transport, count, out, bound, kinds, attempts, settings, files, lead=LEAD,
             show=load.say, clock=time.time, sleep=time.sleep, poll=None, prime_timeout=3600):
    """Funding, the shares, the floods from the workers and the controls:
    the rows floods.build_record judges, and each worker's summary."""
    out = Path(out)
    check_plan(count, kinds, transport.separate)
    wallets = run.role('load')
    if len(wallets) < bound + floods.FLOOD_EXTRA:
        raise Failed(f'the transaction flood needs {bound + floods.FLOOD_EXTRA} load wallets for a queue of {bound}')
    run.fund()
    funded = {w.label: (w.drt, w.dgt) for w in run.wallets.values()}
    flood_labels = {w.label for w in wallets[:bound + floods.FLOOD_EXTRA]}
    shares = load_workers.split(dict(fleet, wallets=[w for w in fleet['wallets'] if w['label'] in flood_labels]),
                                count)
    workers = Workers(transport, count, poll or transport.poll, clock, sleep)
    rows, summary = [], []
    try:
        for n, share in enumerate(shares):
            shared = {key: getattr(settings, key) for key in load_workers.SHARED}
            shared.update(seed=settings.seed + n)
            load_workers.write_shard(fleet, share, n, out / 'shards' / f'{n:02}', funded, shared)
            transport.ship(n, out / 'shards' / f'{n:02}', files)
            transport.start(n)
        show(f'== {count} flood workers started; priming')
        primed = load_workers.wait_for(transport, count, 'primed.json', prime_timeout, clock, sleep, workers.poll)
        sent_at = []
        if 'transaction_flood' in kinds:
            for attempt in range(attempts):
                for _ in range(floods.MAX_TRIES):
                    observed, heads = machine_transaction_flood(run, rpc, workers, bound, lead, show)
                    row = floods.finish('transaction_flood', attempt, observed)
                    rows.append(row)
                    sent_at.append(heads)
                    if row['outcome'] != 'transport':
                        break
        if 'request_flood' in kinds:
            for attempt in range(attempts):
                for _ in range(floods.MAX_TRIES):
                    row = floods.finish('request_flood', attempt,
                                        machine_request_flood(rpc, workers, floods.FLOODERS, lead, show))
                    rows.append(row)
                    if row['outcome'] != 'transport':
                        break
        workers.ask([{'do': 'stop'}] * count, 300)
        summary = [{'shard': p['shard'], 'wallets': p['wallets'], 'primed_s': p['primed_s'],
                    'address': transport.address(n),
                    'sent_at_heads': [h[n] for h in sent_at]} for n, p in enumerate(primed)]
    except BaseException as error:
        load_workers.write_json(out / 'failure.json', {'error': f'{type(error).__name__}: {error}'[:500]})
        raise
    finally:
        transport.stop()
    run.sync([wallets[0]])
    rows.append(floods.control(run, wallets[0], run.funder))
    rows.append(floods.control(run, wallets[0], run.funder))
    return rows, summary


def drive(cli, home, fleet_dir, out, application_config, funder_label, validator, native, engine, transport,
          count, attempts=floods.TARGET, kinds=tuple(floods.REASONS), worker_cli=None, lead=LEAD, show=load.say,
          passphrase=None, settings=None):
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
    files = load_workers.worker_files(worker_cli or cli, home, application_config)
    files.update({name: HERE / name for name in TOOLS})
    settings = settings or load.Settings()
    started = floods.stamp()
    gateway = load.Gateway(cli, env, out / 'gateway.log')
    try:
        rpc = load.Rpc(gateway.url, load_workers.PER_ADDRESS)
        run = load.Load(rpc, load.Signer(cli, env, out / 'scratch'), env, cli, fleet, out, settings, funder,
                        validator, config, show=show)
        rows, summary = campaign(run, rpc, fleet, transport, count, out, bound, kinds, attempts, settings, files,
                                 lead, show)
    finally:
        gateway.stop()
    record = floods.build_record(chain_id, native, engine, rows, attempts, bound, started, floods.stamp())
    (out / 'floods.jsonl').write_bytes(floods.rows_bytes(rows))
    (out / 'floods.json').write_text(json.dumps(record, indent=2) + '\n')
    (out / 'workers.json').write_text(json.dumps({'workers': summary, 'lead_s': lead}, indent=2) + '\n')
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    d = commands.add_parser('drive')
    w = commands.add_parser('work')
    for p in (d, w):
        for name in ('cli', 'home', 'out', 'application_config'):
            p.add_argument(f'--{name.replace("_", "-")}', type=Path, required=True)
        p.add_argument('--validator', required=True)
    w.add_argument('--shard', type=Path, required=True)
    for name in ('fleet', 'genesis', 'engine_genesis'):
        d.add_argument(f'--{name.replace("_", "-")}', type=Path, required=True)
    d.add_argument('--funder', required=True)
    d.add_argument('--workers', type=int, required=True)
    where = d.add_mutually_exclusive_group(required=True)
    where.add_argument('--machines', type=Path, help='the load machines\' run folder (load_machines.py up)')
    where.add_argument('--local', action='store_true')
    d.add_argument('--worker-cli', type=Path, help='the release CLI the load machines run (linux/amd64)')
    d.add_argument('--namespaces', default='')
    d.add_argument('--addresses', default='')
    d.add_argument('--attempts', type=int, default=floods.TARGET)
    d.add_argument('--types', default=','.join(floods.REASONS))
    d.add_argument('--lead', type=float, default=LEAD)
    d.add_argument('--passphrase', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'work':
            work(args.cli, args.home, args.shard, args.out, args.application_config, args.validator)
            return 0
        kinds = tuple(k for k in args.types.split(',') if k)
        unknown = set(kinds) - set(floods.REASONS)
        if unknown:
            raise Failed(f'not flood types: {sorted(unknown)}')
        if args.machines:
            transport = SshTransport(args.machines, args.validator)
            if args.workers > len(transport.machines):
                raise Failed(f'{args.workers} workers but {len(transport.machines)} machines')
        else:
            spaces = [s for s in args.namespaces.split(',') if s]
            addresses = [a for a in args.addresses.split(',') if a]
            if spaces and len(spaces) != args.workers:
                raise Failed('one namespace per worker')
            transport = LocalTransport(args.out / 'workers', args.validator, spaces, addresses)
        record = drive(args.cli, args.home, args.fleet, args.out, args.application_config, args.funder,
                       args.validator, args.genesis.read_bytes(), args.engine_genesis.read_bytes(), transport,
                       args.workers, args.attempts, kinds, args.worker_cli, args.lead, passphrase=args.passphrase)
        floods.report(record)
        return 0 if record['status'] == floods.PASS else 1
    except (floods.Failed, Failed, OSError, KeyError, ValueError, subprocess.SubprocessError) as error:
        print(f'flood_workers: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
