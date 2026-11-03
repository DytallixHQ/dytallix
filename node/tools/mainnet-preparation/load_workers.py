#!/usr/bin/env python3
"""Load from several machines (simulation plan, T3; the load machines,
launch/approvals/P01_T06_LOAD_MACHINES_HETZNER_2026-11-03.json). The sentry's
client channel takes 16 connections, 2 per address, so full capacity needs
several senders, each with its own address. A coordinator on the founder's
machine keeps the funder and the judgement; each worker holds only its share
of throwaway wallets.

  load_workers.py drive --cli DYTALLIX --home HOME --fleet DIR --out DIR
                        --application-config FILE --funder WALLET --validator ID
                        --workers N (--machines RUN_DIR [--worker-cli DYTALLIX]
                                     | --local [--namespaces NS,... --addresses A,...])
                        [--rate TX_PER_S] [--duration S] [--mix KIND=WEIGHT,...] ...
  load_workers.py work --cli DYTALLIX --home HOME --shard DIR --out DIR
                       --application-config FILE --validator ID

drive, with HOME holding the CLI's chain pin and the funder's keystore:
  1. reads the status view (the ledger starts at its height) and checks the
     supply rule;
  2. funds the fleet's load wallets from the funder, through its
     distributors, as load.py run does; the funder never leaves this machine;
  3. splits the load wallets and the spare keys into one disjoint share per
     worker: a share's sends and created accounts stay inside it, so no
     wallet's nonce is tracked in two places;
  4. ships each worker its share (throwaway keys only), the release CLI, the
     load tools, the application config and a CLI home holding only the
     chain pin (chain.json), and starts it;
  5. each worker primes its wallets (their first spends) and reports primed;
     once every worker has, drive sends one start time, and each runs the
     measured window at the total rate over N from it, drains, and writes
     its rows (the --rate given is the total);
  6. drive collects every worker's rows, waits two blocks past the last
     commit, reads the status view and the block headers, and judges the run
     on all rows at once with load.py's evidence: one evidence.json and
     transactions.jsonl that `load.py verify` checks, with a workers section
     in its window (each worker's address, rows digest, start lateness,
     window, and signing and read times).

A worker never judges: its own status views would count the other workers'
burns. drive makes no channel request between the start and the last
worker's rows, and the workers' connections must fit the channel: N times
--connections at most 16, and at most 2 each when each has its own address
(at most 2 in all when they share this machine's). A worker that fails or
stops fails the run: drive stops the others and records what each reported
in failure.json.

Transports. --machines takes the load machines' run folder (load_machines.py
up: machines.json and the run's throwaway SSH key) and pins each machine's
host key on first contact in that folder, never in ~/.ssh; --worker-cli is
the release CLI the machines run (linux/amd64). --local runs the workers as
processes here, each in its own folder, and with --namespaces each in its
own network namespace and address (the CI proof). The evidence names the
workers' addresses and the endpoint, so it stays off the repository; only
address-free summaries are published.
"""
import argparse
from collections import Counter
from datetime import datetime, timezone
import io
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tarfile
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import load  # noqa: E402

SHARD_SCHEMA = 'dytallix.load-shard.v1'
WORKER_SCHEMA = 'dytallix.load-worker.v1'
# The sentry's production bounds on its client channel.
CHANNEL_CONNECTIONS, PER_ADDRESS = 16, 2
# What a worker runs: load.py imports config_checks, which imports native_checks.
TOOLS = ('load.py', 'load_workers.py', 'config_checks.py', 'native_checks.py')
# The settings a worker takes from its share; its rate is the total over N.
SHARED = ('rate', 'duration', 'mix', 'connections', 'batch', 'signers', 'expiry_blocks', 'block_time',
          'poll_interval', 'seed')
REMOTE = '/root/load'

Failed = load.Failed


def iso(seconds):
    return datetime.fromtimestamp(seconds, timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')


def write_json(path, value):
    """Written whole or not at all: the other side polls for the name."""
    path = Path(path)
    temporary = path.with_name(path.name + '.tmp')
    temporary.write_text(json.dumps(value, indent=1) + '\n')
    os.replace(temporary, path)


def budget(workers, connections, separate):
    """The workers' channel connections must fit the sentry's bounds."""
    if workers < 1:
        raise Failed('at least one worker')
    if separate and connections > PER_ADDRESS:
        raise Failed(f'each worker\'s address may hold {PER_ADDRESS} channel connections, not {connections}')
    if not separate and workers * connections > PER_ADDRESS:
        raise Failed(f'workers sharing this machine\'s address may hold {PER_ADDRESS} connections in all, '
                     f'not {workers} x {connections}')
    if workers * connections > CHANNEL_CONNECTIONS:
        raise Failed(f'{workers} workers x {connections} connections exceed the channel\'s {CHANNEL_CONNECTIONS}')


def split(fleet, count):
    """Disjoint shares of the load wallets and the spare keys, in turn."""
    loads = [w for w in fleet['wallets'] if w['role'] == 'load']
    spares = [w for w in fleet['wallets'] if w['role'] == 'spare']
    if count < 1 or len(loads) < 2 * count:
        raise Failed(f'{len(loads)} load wallets cannot give {count} workers two each')
    shares = [loads[n::count] + spares[n::count] for n in range(count)]
    labels = [w['label'] for share in shares for w in share]
    if len(labels) != len(set(labels)):
        raise Failed('two shares hold the same wallet')
    return shares


def write_shard(fleet, share, index, directory, funded, settings):
    """A worker's share: its wallets (with what each was funded) and their
    key files, and the settings it runs."""
    directory = Path(directory)
    (directory / 'keys').mkdir(parents=True, mode=0o700)
    wallets = []
    for record in share:
        load.private_file(directory / record['key_file'], (Path(fleet['_dir']) / record['key_file']).read_bytes())
        drt, dgt = funded.get(record['label'], (0, 0))
        wallets.append(dict(record, funded_udrt=str(drt), funded_udgt=str(dgt)))
    write_json(directory / 'fleet.json', {'schema': SHARD_SCHEMA, 'chain_id': fleet['chain_id'], 'shard': index,
                                          'wallets': wallets, 'settings': settings})


# The worker

def run_worker(run, shard, out, clock=time.time, sleep=time.sleep, poll=0.5, start_timeout=3600, endpoint=None):
    """Primes the share's wallets, reports primed, waits for the one start
    time, runs the measured window and writes its rows and worker.json,
    which the coordinator waits for."""
    out = Path(out)
    for record in shard['wallets']:
        wallet = run.wallets[record['label']]
        wallet.drt, wallet.dgt = int(record['funded_udrt']), int(record['funded_udgt'])
    mark = clock()
    run.first_spends()
    primed_s = round(clock() - mark, 1)
    write_json(out / 'primed.json', {'schema': WORKER_SCHEMA, 'shard': shard['shard'], 'primed_s': primed_s})
    deadline = clock() + start_timeout
    while not (out / 'start.json').exists():
        if clock() > deadline:
            raise Failed(f'no start time arrived within {start_timeout} s')
        sleep(poll)
    start_at = float(json.loads((out / 'start.json').read_bytes())['start_at'])
    while clock() < start_at:
        sleep(min(poll, max(0.0, start_at - clock())))
    began = clock()
    window = run.window()
    ended = clock()
    rows = load.rows_jsonl(run.rows)
    (out / 'transactions.jsonl').write_bytes(rows)
    record = {'schema': WORKER_SCHEMA, 'shard': shard['shard'], 'wallets': len(shard['wallets']),
              'rows': len(run.rows), 'rows_sha256': load.sha256(rows), 'primed_s': primed_s,
              'start_at': iso(start_at), 'late_s': round(began - start_at, 3),
              'window': {'started': iso(began), 'ended': iso(ended), 'duration_s': round(ended - began, 1),
                         'skipped_launches': window['skipped_launches']},
              'rate': run.s.rate, 'connections': run.s.connections, 'endpoint': endpoint,
              'read_ms': {label: load.distribution(values) for label, values in sorted(run.rpc.latency.items())},
              'sign_ms': load.distribution(getattr(run.signer, 'sign_ms', [])), 'cpus': os.cpu_count()}
    write_json(out / 'worker.json', record)
    return record


def work(cli, home, shard_dir, out, application_config, validator, show=load.say):
    """A worker on a load machine: its share's wallets through its own
    gateway and connections; an error is left in error.json."""
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    try:
        shard = json.loads((Path(shard_dir) / 'fleet.json').read_bytes())
        if shard.get('schema') != SHARD_SCHEMA:
            raise Failed(f'{shard_dir}/fleet.json is not {SHARD_SCHEMA}')
        settings = load.Settings(**dict(shard['settings'], governance=False))
        env = dict(os.environ, HOME=str(home))
        config = json.loads(Path(application_config).read_bytes())
        fleet = {'_dir': str(shard_dir), 'chain_id': shard['chain_id'], 'wallets': shard['wallets']}
        gateway = load.Gateway(cli, env, out / 'gateway.log')
        try:
            show(f'== worker {shard["shard"]}: {len(fleet["wallets"])} wallets through {gateway.endpoint}')
            rpc = load.Rpc(gateway.url, settings.connections)
            signer = load.Signer(cli, env, out / 'scratch')
            run = load.Load(rpc, signer, env, cli, fleet, out, settings, None, validator, config, show=show)
            record = run_worker(run, shard, out, endpoint=gateway.endpoint)
            shutil.rmtree(out / 'scratch', ignore_errors=True)
            return record
        finally:
            gateway.stop()
    except Exception as error:
        write_json(out / 'error.json', {'error': f'{type(error).__name__}: {error}'[:500]})
        raise


# Transports: where the workers run

class LocalTransport:
    """Workers as processes on this machine, each in its own folder, and with
    namespaces each in its own network namespace and address."""

    def __init__(self, root, validator, namespaces=(), addresses=(), argv=None):
        self.root, self.validator = Path(root), validator
        self.namespaces, self.addresses = list(namespaces), list(addresses)
        self.separate = bool(self.namespaces)
        self.poll = 2.0
        self.argv = argv or self.worker_argv
        self.processes = {}

    def folder(self, n):
        return self.root / f'worker-{n:02}'

    def address(self, n):
        return self.addresses[n] if n < len(self.addresses) else None

    def worker_argv(self, n):
        f = self.folder(n)
        return [sys.executable, '-I', '-B', str(f / 'load_workers.py'), 'work', '--cli', str(f / 'dytallix'),
                '--home', str(f / 'home'), '--shard', str(f / 'shard'), '--out', str(f / 'out'),
                '--application-config', str(f / 'application-config.json'), '--validator', self.validator]

    def ship(self, n, shard_dir, files):
        f = self.folder(n)
        (f / 'out').mkdir(parents=True)
        for name, source in files.items():
            (f / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, f / name)
        shutil.copytree(shard_dir, f / 'shard')

    def command(self, n):
        prefix = ['ip', 'netns', 'exec', self.namespaces[n]] if self.separate else []
        return prefix + self.argv(n)

    def start(self, n):
        with open(self.folder(n) / 'work.log', 'ab') as log:
            self.processes[n] = subprocess.Popen(self.command(n), cwd=self.folder(n), stdin=subprocess.DEVNULL,
                                                 stdout=log, stderr=subprocess.STDOUT, start_new_session=True)

    def read(self, n, name):
        path = self.folder(n) / 'out' / name
        return path.read_bytes() if path.exists() else None

    def write(self, n, name, value):
        write_json(self.folder(n) / 'out' / name, value)

    def alive(self, n):
        return n in self.processes and self.processes[n].poll() is None

    def stop(self):
        for process in self.processes.values():
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(10)
                except subprocess.TimeoutExpired:
                    process.kill()


class SshTransport:
    """The load machines (load_machines.py up): each worker in REMOTE on its
    own machine, reached as root with the run's throwaway key; host keys are
    pinned on first contact in the run folder, never in ~/.ssh."""

    def __init__(self, run_dir, validator, remote=REMOTE, ssh='ssh', run=subprocess.run):
        run_dir = Path(run_dir)
        state = json.loads((run_dir / 'machines.json').read_bytes())
        self.machines, self.validator, self.remote = state['machines'], validator, remote
        self.key, self.known = run_dir / 'id_ed25519', run_dir / 'known_hosts'
        self.ssh, self._run = ssh, run
        # Each poll is an SSH session per worker.
        self.separate, self.poll = True, 5.0

    def address(self, n):
        return self.machines[n]['ipv4']

    def call(self, n, command, data=b'', check=True):
        argv = [self.ssh, '-i', str(self.key), '-o', f'UserKnownHostsFile={self.known}', '-o',
                'StrictHostKeyChecking=accept-new', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=15', '-o',
                'ServerAliveInterval=15', f'root@{self.address(n)}', command]
        done = self._run(argv, input=data, capture_output=True, timeout=900)
        if check and done.returncode != 0:
            raise Failed(f'worker {n} ({self.machines[n]["name"]}): {(done.stderr or b"").decode(errors="replace").strip()[:300]}')
        return done

    def ship(self, n, shard_dir, files):
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode='w') as archive:
            for name, source in sorted(files.items()):
                archive.add(source, arcname=name)
            archive.add(shard_dir, arcname='shard')
        r = shlex.quote(self.remote)
        self.call(n, f'rm -rf {r} && mkdir -p {r}/out && tar -x -C {r}', buffer.getvalue())

    def start(self, n):
        r = shlex.quote(self.remote)
        argv = ['python3', '-I', '-B', 'load_workers.py', 'work', '--cli', './dytallix', '--home', './home',
                '--shard', './shard', '--out', './out', '--application-config', './application-config.json',
                '--validator', self.validator]
        self.call(n, f'cd {r} && (setsid nohup {shlex.join(argv)} > work.log 2>&1 < /dev/null &)')

    def read(self, n, name):
        done = self.call(n, f'cat {shlex.quote(self.remote + "/out/" + name)}', check=False)
        return done.stdout if done.returncode == 0 else None

    def write(self, n, name, value):
        path = shlex.quote(f'{self.remote}/out/{name}')
        self.call(n, f'cat > {path}.tmp && mv {path}.tmp {path}', (json.dumps(value) + '\n').encode())

    def alive(self, n):
        return self.call(n, 'pgrep -f "load_workers.py work" > /dev/null', check=False).returncode == 0

    def stop(self):
        for n in range(len(self.machines)):
            self.call(n, 'pkill -f "load_workers.py work" || true', check=False)


# The coordinator

def wait_for(transport, count, name, timeout, clock=time.time, sleep=time.sleep, poll=2.0):
    """Every worker's NAME, or the first worker that failed or stopped."""
    found, deadline = {}, clock() + timeout
    while len(found) < count:
        for n in range(count):
            if n in found:
                continue
            raw = transport.read(n, name)
            if raw is not None:
                found[n] = json.loads(raw)
                continue
            error = transport.read(n, 'error.json')
            if error is not None:
                raise Failed(f'worker {n} failed: {json.loads(error)["error"]}')
            if not transport.alive(n):
                raise Failed(f'worker {n} stopped without {name}')
        if len(found) < count:
            if clock() > deadline:
                raise Failed(f'{count - len(found)} workers wrote no {name} within {timeout} s')
            sleep(poll)
    return [found[n] for n in range(count)]


def coordinate(run, fleet, transport, count, out, settings, files, show=load.say, clock=time.time,
               sleep=time.sleep, lead=15.0, prime_timeout=3600, poll=None, started=None):
    """Funding, the shares and the workers, then the evidence over all rows.
    run is the coordinator's Load over the whole fleet (the funder's view)."""
    out = Path(out)
    budget(count, settings.connections, transport.separate)
    poll = poll or transport.poll
    started = started or load.now_utc()
    before_view = run.status('status')
    if not load.supply_rule(before_view):
        raise Failed(f'the supply rule fails before the run at height {before_view["height"]}')
    (out / 'status-before.json').write_text(json.dumps(before_view, indent=2) + '\n')
    before = load.status_reading(before_view)
    show(f'== the ledger starts at height {before["height"]}')
    mark = clock()
    run.fund()
    funding_s = round(clock() - mark, 1)
    shares = split(fleet, count)
    funded = {w.label: (w.drt, w.dgt) for w in run.wallets.values()}
    try:
        for n, share in enumerate(shares):
            shared = {key: getattr(settings, key) for key in SHARED}
            shared.update(rate=settings.rate / count, seed=settings.seed + n)
            write_shard(fleet, share, n, out / 'shards' / f'{n:02}', funded, shared)
            transport.ship(n, out / 'shards' / f'{n:02}', files)
            transport.start(n)
        show(f'== {count} workers started; priming')
        primed = wait_for(transport, count, 'primed.json', prime_timeout, clock, sleep, poll)
        start_at = clock() + lead
        for n in range(count):
            transport.write(n, 'start.json', {'start_at': start_at})
        show(f'== every worker primed; the window starts at {iso(start_at)} for {settings.duration} s, '
             f'{settings.rate} transactions a second in all')
        timeout = lead + settings.duration + settings.block_time * (settings.expiry_blocks + 6) + 120
        reports = wait_for(transport, count, 'worker.json', timeout, clock, sleep, poll)
        rows = [json.loads(line) for line in load.rows_jsonl(run.rows).splitlines()]
        for n, report in enumerate(reports):
            raw = transport.read(n, 'transactions.jsonl')
            if raw is None or load.sha256(raw) != report['rows_sha256']:
                raise Failed(f'worker {n}\'s rows are not the ones its report names')
            rows += [json.loads(line) for line in raw.splitlines() if line.strip()]
    except BaseException as error:
        write_json(out / 'failure.json', {'error': f'{type(error).__name__}: {error}'[:500]})
        raise
    finally:
        transport.stop()
    workers = [dict(report, address=transport.address(n)) for n, report in enumerate(reports)]
    return finish(run, fleet, rows, before, settings, workers, out, funding_s, max(p['primed_s'] for p in primed),
                  started, sleep)


def finish(run, fleet, rows, before, settings, workers, out, funding_s, priming_s, started, sleep=time.sleep):
    """Two blocks past the last commit, the status view and the headers,
    and the evidence over every row (load.py's, so load.py verify checks it)."""
    committed = [r for r in rows if r['status'] == 'committed']
    last = max([r['block_height'] or r['receipt_seen_height'] for r in committed] or [before['height']])
    while True:
        after_view = run.status('status')
        if int(after_view['height']) >= last + 2:
            break
        sleep(2)
    (out / 'status-after.json').write_text(json.dumps(after_view, indent=2) + '\n')
    after = load.status_reading(after_view)
    run_rows = [r for r in rows if r['phase'] == 'run' and r['submit_height'] is not None]
    low = min([r['submit_height'] for r in run_rows] or [before['height']])
    high = max([r['block_height'] or 0 for r in run_rows] or [low]) + 1
    block_times = {str(h): t for h, t in run.block_times(low, high).items()}
    rows_bytes = b''.join(load.canonical(r) + b'\n' for r in rows)
    (out / 'transactions.jsonl').write_bytes(rows_bytes)
    ledger_record = load.ledger(rows, before['height'], run.creation_fee)
    (out / 'ledger.json').write_text(json.dumps(ledger_record, indent=2) + '\n')
    (out / 'accounts.json').write_text(json.dumps({'addresses': load.created_addresses(rows, run.wallets)},
                                                  indent=2) + '\n')
    fleet_record = {'wallets': Counter(w['role'] for w in fleet['wallets']),
                    'addresses': [w['address'] for w in fleet['wallets']], 'funder': run.funder.address}
    window_record = {'duration_s': max(w['window']['duration_s'] for w in workers),
                     'skipped_launches': sum(w['window']['skipped_launches'] for w in workers),
                     'warmup': {'funding_s': funding_s, 'priming_s': priming_s}, 'sweeps': [],
                     'workers': workers, 'addresses': sorted({w['address'] for w in workers if w['address']}),
                     'read_ms': {label: load.distribution(values) for label, values in sorted(run.rpc.latency.items())},
                     'sign_ms': load.distribution(getattr(run.signer, 'sign_ms', [])), 'cpus': os.cpu_count()}
    settings_record = {'rate': settings.rate, 'duration': settings.duration, 'mix': settings.mix,
                       'connections': settings.connections, 'batch': settings.batch, 'signers': settings.signers,
                       'expiry_blocks': settings.expiry_blocks, 'fund_drt': settings.fund_drt,
                       'fund_dgt': settings.fund_dgt, 'governance': False, 'governance_vote': False,
                       'seed': settings.seed, 'sweep_from': [], 'sweep_interval': settings.sweep_interval,
                       'distributors': sum(w['role'] == 'distributor' for w in fleet['wallets']),
                       'workers': len(workers), 'min_within_2': settings.min_within_2,
                       'max_refused': settings.max_refused}
    record = load.evidence(fleet['chain_id'], settings_record, fleet_record, rows_bytes, ledger_record, before, after,
                           block_times, {}, window_record, started, load.now_utc())
    if load.verify(json.loads(json.dumps(record)), rows_bytes) != record['status']:
        raise Failed('the merged evidence does not verify')
    write_json(out / 'evidence.json', record)
    return record


def worker_files(cli, home, application_config):
    """What each worker gets besides its share: the tools, the release CLI,
    the application config, and a CLI home holding only the chain pin."""
    pin = Path(home) / '.dytallix' / 'chain.json'
    if not pin.exists():
        raise Failed(f'{home} has no pinned chain (dytallix config pin-chain)')
    files = {name: HERE / name for name in TOOLS}
    files.update({'dytallix': Path(cli), 'application-config.json': Path(application_config),
                  'home/.dytallix/chain.json': pin})
    return files


def drive(cli, home, fleet_dir, out, application_config, funder_label, validator, settings, transport, count,
          worker_cli=None, show=load.say, passphrase=None):
    out, fleet_dir = Path(out), Path(fleet_dir)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, HOME=str(home))
    if passphrase:
        env['DYTALLIX_KEYSTORE_PASSPHRASE_FILE'] = str(passphrase)
    config = json.loads(Path(application_config).read_bytes())
    fleet = json.loads((fleet_dir / 'fleet.json').read_bytes())
    fleet['_dir'] = str(fleet_dir)
    entries = {e['name']: e for e in json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())['entries']}
    funder = load.keystore_account(entries, funder_label, fleet['chain_id'])
    files = worker_files(worker_cli or cli, home, application_config)
    started = load.now_utc()
    gateway = load.Gateway(cli, env, out / 'gateway.log')
    show(f'== the gateway relays to {gateway.endpoint} at {gateway.url}')
    try:
        rpc = load.Rpc(gateway.url, settings.connections)
        signer = load.Signer(cli, env, out / 'scratch')
        run = load.Load(rpc, signer, env, cli, fleet, out, settings, funder, validator, config, show=show)
        record = coordinate(run, fleet, transport, count, out, settings, files, show=show, started=started)
        shutil.rmtree(out / 'scratch', ignore_errors=True)
        return record
    finally:
        gateway.stop()


def report(record, show=load.say):
    load.report(record, show=show)
    for w in record['window']['workers']:
        show(f'== worker {w["shard"]} at {w["address"] or "this machine"}: {w["rows"]} rows, started '
             f'{w["late_s"]} s late, window {w["window"]["duration_s"]} s, signing {w["sign_ms"]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    d = commands.add_parser('drive')
    w = commands.add_parser('work')
    for p in (d, w):
        p.add_argument('--cli', type=Path, required=True)
        p.add_argument('--home', type=Path, required=True)
        p.add_argument('--out', type=Path, required=True)
        p.add_argument('--application-config', type=Path, required=True)
        p.add_argument('--validator', required=True)
    w.add_argument('--shard', type=Path, required=True)
    d.add_argument('--fleet', type=Path, required=True)
    d.add_argument('--funder', required=True)
    d.add_argument('--workers', type=int, required=True)
    where = d.add_mutually_exclusive_group(required=True)
    where.add_argument('--machines', type=Path, help='the load machines\' run folder (load_machines.py up)')
    where.add_argument('--local', action='store_true')
    d.add_argument('--worker-cli', type=Path, help='the release CLI the load machines run (linux/amd64)')
    d.add_argument('--namespaces', default='')
    d.add_argument('--addresses', default='')
    d.add_argument('--rate', type=float, default=1.0, help='transactions a second, all workers together')
    d.add_argument('--duration', type=float, default=120.0)
    d.add_argument('--mix', default=load.DEFAULT_MIX)
    d.add_argument('--seed', type=int, default=1)
    d.add_argument('--connections', type=int, default=2)
    d.add_argument('--expiry-blocks', type=int, default=20)
    d.add_argument('--block-time', type=float, default=5.0)
    d.add_argument('--fund-drt', type=int, default=200)
    d.add_argument('--fund-dgt', type=int, default=500)
    d.add_argument('--min-within-2', type=float, default=0.99)
    d.add_argument('--max-refused', type=int)
    d.add_argument('--passphrase', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'work':
            work(args.cli, args.home, args.shard, args.out, args.application_config, args.validator)
            return 0
        settings = load.Settings(rate=args.rate, duration=args.duration, mix=load.parse_mix(args.mix), seed=args.seed,
                                 connections=args.connections, expiry_blocks=args.expiry_blocks,
                                 block_time=args.block_time, fund_drt=args.fund_drt * load.UNIT,
                                 fund_dgt=args.fund_dgt * load.UNIT, min_within_2=args.min_within_2,
                                 max_refused=args.max_refused)
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
                       args.validator, settings, transport, args.workers, args.worker_cli, passphrase=args.passphrase)
        report(record)
        return 0 if record['status'] == load.PASS else 1
    except (Failed, OSError, KeyError, ValueError, subprocess.SubprocessError) as error:
        print(f'load_workers: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
