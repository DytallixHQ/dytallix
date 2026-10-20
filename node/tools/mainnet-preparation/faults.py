#!/usr/bin/env python3
"""The fault injector (simulation plan, T5): it injects a fault on the sentry
through the signed staging agent, watches the node degrade and then recover,
and writes digest-bound evidence that `faults.py verify` re-evaluates from the
file alone.

  faults.py verify --evidence FILE --genesis NATIVE_GENESIS \
      --engine-genesis ENGINE_GENESIS

Each scenario records the agent command that caused the fault (and the clear
that removed it), proof the fault took hold, the sentry's height before and
after, and the guest-clock instants the fault was cleared and the node
recovered. A scenario is a FAIL when the fault or its clear did not run, when
it had no measurable effect (so a no-op cannot pass), when finalized history
was lost (the height fell), or when the node never caught back up. Otherwise
the recovery time is measured and judged against the approved recovery target;
with no target set yet (an open P01 decision, `launch-rehearsals.json`
dependency "recovery target") the scenario is INCONCLUSIVE, which no run counts
as a pass. The target the record was built with is covered by the digest, so it
cannot be edited after the fact.

  faults.py run --store STORE.json --chain ID --host LABEL --cli DYTALLIX --home DIR \
      --key KEY.json --paper FILE --signer DYTALLIX_ROOT_SIGN --work DIR \
      --status-url URL --genesis NATIVE_GENESIS --engine-genesis ENGINE_GENESIS \
      --target SECONDS --out FILE

run drives the scenarios on a real host from the founder's machine (T2's
real-host harness), through only the public paths: signed commands and sealed
results in the command store (staging_founder.py) and the sentry's public
status page. The CI fault drill runs the same `run_scenarios`. Each fault
command's last step is a read-only proof (the qdisc, the nftables set, the
unit's state) that comes back in the same signed result (P01, 14 October
2026). Recovery is timed on the status page, polled about once a second from
the founder's machine (P01, 14 October 2026): the fault clears at the host's
clock (the injection's finish plus its duration, when the host's own dead-man
removes it; the restart's finish after a process kill), and the sentry has
recovered when the page serves a block made at or after that instant, above
the height it served at the clear, read at the founder's clock. Each
scenario records which clock each instant came from.

The lean scope is the network and process faults driven through the agent:
latency and loss (`tc netem`) and a peer partition (an `inet dytallix_fault`
nftables table, scoped to the p2p port so the agent's own command path stays
reachable), and a process kill. Peer drops and host loss follow (they need a VM
restart path). The validator keeps producing throughout; recovery is the
sentry catching back up, read through the guest agent, whose virtio serial
survives the fault that the network path does not.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sys
import threading
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import reconcile  # noqa: E402

SCHEMA = 'dytallix.fault-run.v1'
PASS, FAIL, INCONCLUSIVE = reconcile.PASS, reconcile.FAIL, reconcile.INCONCLUSIVE

# A scenario's kind decides how its effect is proven. The netem kinds confirm a
# netem qdisc was active; the partition confirms the sentry stopped advancing
# while cut off; the process kind confirms the node was down right after the
# kill. verify recomputes the kind from the name, so a record cannot file a
# scenario under a softer kind.
KINDS = {'latency': 'netem', 'loss': 'netem', 'partition': 'partition', 'process_kill': 'process'}


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def instant(text):
    return datetime.fromisoformat(text.replace('Z', '+00:00'))


def is_int(value):
    return type(value) is int


# The scenarios run on the sentry. The network faults self-expire on the host
# (a netem dead-man timer, an nftables timeout set), so a console-only host is
# never locked out.
SCENARIOS = (
    {'name': 'latency', 'command': 'fault-netem',
     'args': {'delay_ms': 400, 'loss_percent': 5, 'duration_seconds': 45}},
    {'name': 'loss', 'command': 'fault-netem',
     'args': {'delay_ms': 50, 'loss_percent': 40, 'duration_seconds': 45}},
    {'name': 'partition', 'command': 'fault-partition', 'args': {'duration_seconds': 90}},
    {'name': 'process_kill', 'command': 'fault-kill', 'args': {}},
)
CLOCKS = {'injected_at': 'host', 'cleared_at': 'host', 'recovered_at': 'reader'}


def kind(name):
    try:
        return KINDS[name]
    except KeyError:
        raise Failed(f'{name!r} is not a registered fault scenario')


def recovery_seconds(scenario):
    return (instant(scenario['recovered_at']) - instant(scenario['cleared_at'])).total_seconds()


def effect_holds(scenario_kind, effect):
    """Whether the recorded effect proves the fault took hold."""
    if not effect:
        return False
    if scenario_kind == 'netem':
        return 'netem' in (effect.get('qdisc') or '')
    if scenario_kind == 'partition':
        # The drop is objectively installed (deterministic, unlike watching for a
        # freeze, which races TCP teardown); recovery then proves it was real.
        return effect.get('installed') is True
    if scenario_kind == 'process':
        return (effect.get('active') or '') in ('inactive', 'failed', 'deactivating')
    return False


def evaluate(scenario, target):
    """The status and detail of one scenario, derived only from its recorded
    observations. Never reads the recorded status."""
    this = kind(scenario['name'])
    command = scenario.get('command') or {}
    if command.get('status') != 'DONE':
        return FAIL, 'the fault command did not run to completion'
    if not effect_holds(this, scenario.get('effect') or {}):
        return FAIL, 'the fault had no measurable effect'
    for field in ('baseline_height', 'height_at_clear'):
        if not is_int(scenario.get(field)):
            return INCONCLUSIVE, f'no {field} was read'
    baseline, at_clear = scenario['baseline_height'], scenario['height_at_clear']
    recovered = scenario.get('recovered_height')
    if scenario.get('recovered_at') is None or not is_int(recovered):
        return FAIL, 'the node did not catch back up within the drill timeout'
    if recovered < baseline:
        return FAIL, f'finalized history was lost: the height fell from {baseline} to {recovered}'
    if recovered <= at_clear:
        return FAIL, f'the chain did not advance after the fault cleared (held at {recovered})'
    seconds = recovery_seconds(scenario)
    if seconds < 0:
        return FAIL, 'the recovery instant precedes the clear'
    if target is None:
        return INCONCLUSIVE, (f'recovered {recovered - at_clear} blocks in {seconds:.0f} s; '
                              'awaits the approved recovery target (open P01 decision)')
    if seconds <= target:
        return PASS, f'recovered in {seconds:.0f} s, within the {target} s target'
    return FAIL, f'recovered in {seconds:.0f} s, over the {target} s target'


# Driving the scenarios through the public paths

def effect_of(name, result):
    """The proof a fault took hold: its command's last, read-only step, in the
    same signed result. The shape is the one effect_holds reads."""
    steps = result.get('steps') or []
    out = (steps[-1].get('stdout') or '').strip() if steps else ''
    this = kind(name)
    if this == 'netem':
        return {'qdisc': out}
    if this == 'partition':
        return {'installed': '26656' in out, 'set': out}
    return {'active': out}


def iso_at(seconds):
    return datetime.fromtimestamp(seconds, tz=timezone.utc).isoformat()


class Timeline:
    """The status page read about once a second in the background."""

    def __init__(self, read, sleep=time.sleep, period=1.0):
        self.read, self.sleep, self.period = read, sleep, period
        self.pages, self.stopped = [], threading.Event()
        self.thread = threading.Thread(target=self.loop, daemon=True)

    def loop(self):
        while not self.stopped.is_set():
            self.pages.append(self.read())
            self.sleep(self.period)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.stopped.set()
        self.thread.join(timeout=30)

    def last_height(self, before=None):
        """The newest height served (at or before an instant, if given)."""
        heights = [p['height'] for p in list(self.pages) if isinstance(p.get('height'), int)
                   and (before is None or instant(p['read_at']).timestamp() <= before)]
        return heights[-1] if heights else None

    def recovered(self, cleared_at, above, made):
        """The first page read after the clear that serves a block made at or
        after it, above the height at the clear: (read_at, height) or None."""
        for page in list(self.pages):
            when = made(page)
            if (page.get('http') == 200 and when is not None and when >= cleared_at
                    and isinstance(page.get('height'), int) and (above is None or page['height'] > above)
                    and instant(page['read_at']).timestamp() >= cleared_at):
                return page['read_at'], page['height']
        return None


def run_scenarios(channel, read_page, made, scenarios=SCENARIOS, result_timeout=600, recovery_timeout=600,
                  clock=time.time, sleep=time.sleep, say=print, period=1.0, on_cleared=None):
    """Each scenario through the agent channel, timed on the status page.
    `read_page()` is one status page reading (staging_founder.status_page);
    `made(page)` its block time in seconds. `on_cleared(name)`, if given, runs
    once a network fault's dead-man time has passed (the CI drill checks there
    that the host removed it). Returns the raw observations."""
    rows = []
    for spec in scenarios:
        name, command, args = spec['name'], spec['command'], dict(spec['args'])
        say(f'== fault scenario: {name}')
        with Timeline(read_page, sleep, period) as timeline:
            began = clock()
            while timeline.last_height() is None:
                if clock() > began + recovery_timeout:
                    raise Failed(f'{name}: the status page served no height')
                sleep(period)
            baseline = timeline.last_height()
            if command == 'fault-kill':
                kill, restart = channel.send(command, args), channel.send('start')
                injected, started = channel.wait([kill, restart], result_timeout)
                cleared_at, clear = started.get('finished_at'), {'mechanism': 'restart', 'status': started['status']}
            else:
                injected = channel.wait([channel.send(command, args)], result_timeout)[0]
                finished = injected.get('finished_at')
                cleared_at = finished + args['duration_seconds'] if finished is not None else None
                clear = {'mechanism': 'dead-man'}
            row = {'name': name, 'command': {'command': command, 'args': args, 'status': injected['status']},
                   'clear': clear, 'effect': effect_of(name, injected), 'baseline_height': baseline,
                   'clocks': dict(CLOCKS), 'injected_at': None, 'cleared_at': None, 'height_at_clear': None,
                   'recovered_at': None, 'recovered_height': None}
            if injected.get('finished_at') is not None:
                row['injected_at'] = iso_at(injected['finished_at'])
            if injected['status'] == 'DONE' and cleared_at is not None:
                row['cleared_at'] = iso_at(cleared_at)
                deadline = cleared_at + recovery_timeout
                while clock() < cleared_at + 2:
                    sleep(period)
                row['height_at_clear'] = timeline.last_height(before=cleared_at)
                if on_cleared and command != 'fault-kill':
                    on_cleared(name)
                while True:
                    found = timeline.recovered(cleared_at, row['height_at_clear'], made)
                    if found or clock() > deadline:
                        break
                    sleep(period)
                if found:
                    row['recovered_at'], row['recovered_height'] = found
        if command != 'fault-kill':
            # The dead-man already removed it; this removes the empty table.
            clear['status'] = channel.run('fault-clear', timeout=result_timeout)['status']
        say(f'== {name}: {"caught back up at " + str(row["recovered_height"]) if row["recovered_at"] else "no recovery"}')
        rows.append(row)
    return rows


def run_status(statuses):
    if FAIL in statuses:
        return FAIL
    if INCONCLUSIVE in statuses or not statuses:
        return INCONCLUSIVE
    return PASS


def build_scenario(scenario, target):
    row = dict(scenario)
    row['kind'] = kind(scenario['name'])
    row['status'], row['detail'] = evaluate(scenario, target)
    if scenario.get('recovered_at') is not None and scenario.get('cleared_at') is not None:
        row['recovery_seconds'] = recovery_seconds(scenario)
    return row


def build_run(chain_id, genesis_bytes, engine_genesis_bytes, scenarios, target, host):
    """Assemble the run record. `scenarios` are the raw observations; the status
    of each (and of the run) comes from the same `evaluate` that `verify` re-runs,
    so the two always agree. `target` is the approved recovery target in seconds,
    or None while it is undecided."""
    rows = [build_scenario(scenario, target) for scenario in scenarios]
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'host': host,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'recovery_target_seconds': target, 'scenarios': rows,
            'status': run_status([row['status'] for row in rows])}
    body['sha256'] = digest(canonical(body))
    return body


def verify(record, genesis_bytes, engine_genesis_bytes):
    """Re-evaluate a run record from its observations; any edit, a swapped
    genesis, a softened kind, or an edited recovery target shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    if record['genesis']['engine_genesis_sha256'] != digest(engine_genesis_bytes):
        raise Failed('the evidence names another engine genesis')
    target = record.get('recovery_target_seconds')
    if target is not None and not (is_int(target) and target > 0):
        raise Failed('the recovery target is not a positive number of seconds')
    scenarios = record['scenarios']
    if not scenarios:
        raise Failed('the record holds no fault scenarios')
    statuses = []
    for row in scenarios:
        if row.get('kind') != kind(row['name']):
            raise Failed(f'the recorded kind for {row["name"]!r} is wrong')
        status, detail = evaluate(row, target)
        if (row.get('status'), row.get('detail')) != (status, detail):
            raise Failed(f'the recorded scenario {row["name"]!r} does not follow from its observations')
        statuses.append(status)
    if run_status(statuses) != record['status']:
        raise Failed('the recorded run status does not follow from its scenarios')
    return record['status']


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True, help='native-genesis.json')
    v.add_argument('--engine-genesis', type=Path, required=True, help='the engine genesis.json')
    r = commands.add_parser('run')
    for name in ('store', 'cli', 'home', 'key', 'paper', 'signer', 'work', 'genesis', 'engine_genesis', 'out'):
        r.add_argument(f'--{name.replace("_", "-")}', type=Path, required=True)
    r.add_argument('--chain', required=True)
    r.add_argument('--host', required=True)
    r.add_argument('--status-url', required=True)
    r.add_argument('--target', type=int, required=True, help='the approved recovery target in seconds')
    args = parser.parse_args()
    if args.command == 'run':
        return run_main(args)
    try:
        record = json.loads(args.evidence.read_bytes())
        status = verify(record, args.genesis.read_bytes(), args.engine_genesis.read_bytes())
    except (Failed, reconcile.Failed, OSError, KeyError, ValueError) as error:
        print(f'faults: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


def run_main(args):
    import staging_founder
    try:
        store = staging_founder.Store(staging_founder.load_store(args.store), args.work / 'store-scratch')
        channel = staging_founder.Channel(store, args.chain, args.host, args.work, args.cli, args.home, args.key,
                                          args.paper, args.signer)
        probe = channel.run('fault-clear')
        if probe['status'] != 'DONE':
            raise Failed(f'the fault commands do not run on this host (tc, nft or the interface): {probe.get("steps")}')
        rows = run_scenarios(channel, lambda: staging_founder.status_page(args.status_url),
                             staging_founder.block_instant)
        record = build_run(args.chain, args.genesis.read_bytes(), args.engine_genesis.read_bytes(), rows,
                           args.target, args.host)
        args.out.write_text(json.dumps(record, indent=2) + '\n')
        status = verify(json.loads(args.out.read_bytes()), args.genesis.read_bytes(), args.engine_genesis.read_bytes())
    except (Failed, staging_founder.Failed, staging_founder.staging_command.Failed, OSError, KeyError,
            ValueError) as error:
        print(f'faults: {error}', file=sys.stderr)
        return 1
    for row in record['scenarios']:
        print(f'{row["name"]}: {row["status"]} ({row["detail"]})')
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
