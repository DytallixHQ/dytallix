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
      --target SECONDS --out FILE [--scenarios NAME,...]

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
scenario records which clock each instant came from, and the reader's longest
gap without an answer from the sentry's host (a status, or a refused
connection while its node is down) from the clear to the recovery: a gap can
only make a recovery look slower, so a scenario that would fail on its timing
after a gap over READER_GAP_SECONDS (the founder's machine asleep or offline)
is INCONCLUSIVE and is run again, never counted (P01, 4 November 2026).

The lean scope is the network and process faults driven through the agent:
latency and loss (`tc netem`) and a peer partition (an `inet dytallix_fault`
nftables table, scoped to the p2p port so the agent's own command path stays
reachable), a peer drop (the established p2p connections closed once with
`ss -K`; nothing is blocked, so the nodes redial at once; P01, 2 November
2026) and a process kill, on the sentry; the validator keeps producing
throughout, and recovery is the sentry catching back up. The validator's
process restart (R02, `validator_restart`) is the same kill and restart on the
validator, through its own agent, as its own record (`--host validator-1
--scenarios validator_restart`): with one validator the chain halts while it
is down, so recovery is the next block the sentry's status page serves, judged
against the approved 5-minute target (TIMINGS.json
validator_restart_target_seconds). Host loss (`host_loss`) is the sentry's
whole machine powered off hard and on again from the founder's machine, not
through the agent (a host cannot bring itself back): the disk survives, and
recovery is the sentry serving a block made after the power-on, judged against
its own approved 10-minute target (TIMINGS.json host_loss_target_seconds; P01,
9 November 2026). Its injection and clear are timed on the founder's clock,
and its reader gap counts only the reader's own silences (no attempt at all),
since a host that is off or booting answers nothing. It is built for the CI
staging network (the VM's power); the real hosts' power path (the provider's
API) is not built yet.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
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
# while cut off; the peers kind confirms connections were closed and none of
# them is open after; the process kind confirms the node was down right after the
# kill (the sentry's, or the validator's for validator_restart). verify
# recomputes the kind from the name, so a record cannot file a scenario under
# a softer kind.
KINDS = {'latency': 'netem', 'loss': 'netem', 'partition': 'partition', 'peer_drop': 'peers',
         'process_kill': 'process', 'validator_restart': 'process', 'host_loss': 'host'}


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
    {'name': 'peer_drop', 'command': 'fault-peer-drop', 'args': {}},
    # At least 150 s, so the once-a-minute monitor must see the 60 s halt
    # rule fire (P01, 10 November 2026; P01_T05_FAULT_ALERTS_2026-11-10).
    {'name': 'partition', 'command': 'fault-partition', 'args': {'duration_seconds': 150}},
    {'name': 'process_kill', 'command': 'fault-kill', 'args': {}},
)
# R02's validator process restart, on the validator through its own agent.
VALIDATOR_SCENARIOS = (
    {'name': 'validator_restart', 'command': 'fault-kill', 'args': {}},
)
BY_NAME = {spec['name']: spec for spec in SCENARIOS + VALIDATOR_SCENARIOS}
# Host loss (P01, 9 November 2026): the sentry's machine powered off hard, held
# off, then powered on, by the founder's power controller rather than the agent.
# Held off past the alerting service's 3-minute heartbeat rule, so its alert
# must fire (P01, 10 November 2026; P01_T05_FAULT_ALERTS_2026-11-10); the
# recovery target is still timed from the power-on.
HOST_LOSS = {'name': 'host_loss', 'command': 'power-cycle', 'args': {'off_seconds': 210}}
# The kill's own dead-man (P01, 4 November 2026; ALLOWLIST.json fault-kill):
# the host starts the node this long after a kill if nothing else has, so a
# founder machine that drops between the kill and its restart cannot leave
# the node down. The founder's start normally comes first.
KILL_DEAD_MAN_SECONDS = 60
# After a partition or a process kill the nodes redial within 60 s (launch
# rehearsals, One validator). A peer drop sooner finds no connection to close
# and proves nothing (round 1 on the real hosts, 3 November 2026), so it waits
# that long, with a margin, whatever the order.
REDIAL_SECONDS = 75
BREAKS_PEERS = ('partition', 'process_kill')
CLOCKS = {'injected_at': 'host', 'cleared_at': 'host', 'recovered_at': 'reader'}
# Host loss is injected and cleared from the founder's machine, the reader's.
HOST_CLOCKS = {'injected_at': 'reader', 'cleared_at': 'reader', 'recovered_at': 'reader'}
# The reader reads about once a second, each read waiting at most 5 s. On 4
# November 2026 (UTC) the founder's laptop slept with its lid closed during
# rounds 22 and 23, and its late readings failed recoveries that the hosts'
# own clears and the chain show were on time.
READER_GAP_SECONDS = 10


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
    if scenario_kind == 'peers':
        closed, after = effect.get('closed') or [], effect.get('after') or []
        return bool(closed) and not set(closed) & set(after)
    if scenario_kind == 'process':
        return (effect.get('active') or '') in ('inactive', 'failed', 'deactivating')
    if scenario_kind == 'host':
        # The controller saw the machine stop, the reader read while it was
        # off, and the status page served nothing in that time.
        return (effect.get('powered_off') is True and is_int(effect.get('reads_while_off'))
                and effect['reads_while_off'] > 0 and effect.get('served_while_off') == 0)
    return False


def timed_failure(scenario, detail):
    """A failure on the reader's timing (no recovery seen, or seen over the
    target): INCONCLUSIVE after a reader gap over READER_GAP_SECONDS, since
    nobody watched; otherwise FAIL."""
    gap = scenario.get('reader_gap_seconds')
    if isinstance(gap, (int, float)) and not isinstance(gap, bool) and gap > READER_GAP_SECONDS:
        return INCONCLUSIVE, (f'the reader was not watching for {gap:.0f} s (the founder\'s machine asleep or '
                              f'offline), so it cannot judge: {detail}; run it again')
    return FAIL, detail


def evaluate(scenario, target):
    """The status and detail of one scenario, derived only from its recorded
    observations. Never reads the recorded status."""
    this = kind(scenario['name'])
    command = scenario.get('command') or {}
    if command.get('status') != 'DONE':
        return FAIL, 'the fault command did not run to completion'
    # The founder's restart is part of a kill scenario: if it failed, the
    # scenario fails even when the kill's dead-man brought the node back
    # (P01, 4 November 2026), so a failing start is never hidden.
    clear = scenario.get('clear') or {}
    if clear.get('mechanism') == 'restart' and clear.get('status') != 'DONE':
        return FAIL, 'the restart command did not run to completion'
    if clear.get('mechanism') == 'power-on' and clear.get('status') != 'DONE':
        return FAIL, 'the power-on did not run to completion'
    if not effect_holds(this, scenario.get('effect') or {}):
        return FAIL, 'the fault had no measurable effect'
    for field in ('baseline_height', 'height_at_clear'):
        if not is_int(scenario.get(field)):
            return INCONCLUSIVE, f'no {field} was read'
    baseline, at_clear = scenario['baseline_height'], scenario['height_at_clear']
    recovered = scenario.get('recovered_height')
    if scenario.get('recovered_at') is None or not is_int(recovered):
        return timed_failure(scenario, 'the node did not catch back up within the drill timeout')
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
    return timed_failure(scenario, f'recovered in {seconds:.0f} s, over the {target} s target')


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
    if this == 'peers':
        return {'closed': connections(steps[0].get('stdout') if steps else ''), 'after': connections(out)}
    return {'active': out}


def connections(listing):
    """The connections in an `ss -Htn` listing, each its local and peer
    address (the first two ADDRESS:PORT words of a line), sorted."""
    pairs = set()
    for line in (listing or '').splitlines():
        ends = [word for word in line.split() if re.fullmatch(r'\S+:\d+', word)][:2]
        if len(ends) == 2:
            pairs.add(' '.join(ends))
    return sorted(pairs)


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

    def largest_gap(self, start, end, answered_only=True):
        """The longest time from `start` to `end` (seconds) without a reading
        the sentry's host answered: an HTTP status, or a refused connection
        (its node down). A timeout or a network error on the reader's side is
        not an answer. With `answered_only` False (host loss, where the host
        itself is off or booting), any attempt counts: only a reader that did
        not read at all (asleep) leaves a gap."""
        answered = [instant(p['read_at']).timestamp() for p in list(self.pages)
                    if p.get('read_at') and (not answered_only or p.get('http') is not None or p.get('refused'))]
        span = [start] + [t for t in answered if start < t <= end] + [max(end, start)]
        return round(max(b - a for a, b in zip(span, span[1:])), 1)

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


def settle(broke_at, clock=time.time, sleep=time.sleep, say=print):
    """Waits until REDIAL_SECONDS have passed since a scenario that broke
    the peer connections ended (broke_at, or None); returns the wait."""
    wait = 0.0 if broke_at is None else broke_at + REDIAL_SECONDS - clock()
    if wait > 0:
        say(f'== waiting {wait:.0f} s for the nodes to redial before the peer drop')
        sleep(wait)
    return max(0.0, wait)


def run_scenarios(channel, read_page, made, scenarios=SCENARIOS, result_timeout=600, recovery_timeout=600,
                  clock=time.time, sleep=time.sleep, say=print, period=1.0, on_cleared=None):
    """Each scenario through the agent channel, timed on the status page.
    `read_page()` is one status page reading (staging_founder.status_page);
    `made(page)` its block time in seconds. `on_cleared(name)`, if given, runs
    once a network fault's dead-man time has passed (the CI drill checks there
    that the host removed it). Returns the raw observations."""
    rows, broke_at = [], None
    for spec in scenarios:
        name, command, args = spec['name'], spec['command'], dict(spec['args'])
        if name == 'peer_drop':
            settle(broke_at, clock, sleep, say)
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
            elif command == 'fault-peer-drop':
                # The connections close once and nothing stays blocked: the
                # drop clears when the command finishes, and the nodes redial.
                injected = channel.wait([channel.send(command, args)], result_timeout)[0]
                cleared_at, clear = injected.get('finished_at'), {'mechanism': 'redial'}
            else:
                injected = channel.wait([channel.send(command, args)], result_timeout)[0]
                finished = injected.get('finished_at')
                cleared_at = finished + args['duration_seconds'] if finished is not None else None
                clear = {'mechanism': 'dead-man'}
            row = {'name': name, 'command': {'command': command, 'args': args, 'status': injected['status']},
                   'clear': clear, 'effect': effect_of(name, injected), 'baseline_height': baseline,
                   'clocks': dict(CLOCKS), 'injected_at': None, 'cleared_at': None, 'height_at_clear': None,
                   'recovered_at': None, 'recovered_height': None, 'reader_gap_seconds': None}
            if injected.get('finished_at') is not None:
                row['injected_at'] = iso_at(injected['finished_at'])
            if injected['status'] == 'DONE' and cleared_at is not None:
                row['cleared_at'] = iso_at(cleared_at)
                deadline = cleared_at + recovery_timeout
                while clock() < cleared_at + 2:
                    sleep(period)
                row['height_at_clear'] = timeline.last_height(before=cleared_at)
                if on_cleared and clear['mechanism'] == 'dead-man':
                    on_cleared(name)
                while True:
                    found = timeline.recovered(cleared_at, row['height_at_clear'], made)
                    if found or clock() > deadline:
                        break
                    sleep(period)
                if found:
                    row['recovered_at'], row['recovered_height'] = found
                end = instant(found[0]).timestamp() if found else clock()
                row['reader_gap_seconds'] = timeline.largest_gap(cleared_at, end)
        if clear['mechanism'] == 'dead-man':
            # The dead-man already removed it; this removes the empty table.
            clear['status'] = channel.run('fault-clear', timeout=result_timeout)['status']
        say(f'== {name}: {"caught back up at " + str(row["recovered_height"]) if row["recovered_at"] else "no recovery"}')
        rows.append(row)
        if name in BREAKS_PEERS:
            broke_at = clock()
    return rows


def run_host_loss(power, read_page, made, spec=HOST_LOSS, recovery_timeout=900, clock=time.time,
                  sleep=time.sleep, say=print, period=1.0):
    """Host loss (P01, 9 November 2026) through a power controller on the
    founder's machine: `power.off()` stops the machine hard and returns
    {'status', 'finished_at', 'stopped'}; `power.on()` starts it again on the
    same disk and returns {'status', 'finished_at'}. Recovery is timed on the
    status page as for the other faults, from the power-on. Returns the raw
    observation."""
    name, off_seconds = spec['name'], spec['args']['off_seconds']
    say(f'== fault scenario: {name} (powered off for {off_seconds} s)')
    with Timeline(read_page, sleep, period) as timeline:
        began = clock()
        while timeline.last_height() is None:
            if clock() > began + recovery_timeout:
                raise Failed(f'{name}: the status page served no height')
            sleep(period)
        baseline = timeline.last_height()
        off = power.off()
        off_at = off.get('finished_at')
        if off['status'] == 'DONE' and off_at is not None:
            while clock() < off_at + off_seconds:
                sleep(period)
            on = power.on()
        else:
            on = {'status': 'NOT_RUN', 'finished_at': None}
        cleared_at = on.get('finished_at')
        pages = [p for p in list(timeline.pages) if p.get('read_at') and off_at is not None
                 and cleared_at is not None and off_at <= instant(p['read_at']).timestamp() < cleared_at]
        row = {'name': name, 'command': {'command': spec['command'], 'args': dict(spec['args']),
                                         'status': off['status']},
               'clear': {'mechanism': 'power-on', 'status': on['status']},
               'effect': {'powered_off': off.get('stopped') is True, 'reads_while_off': len(pages),
                          'served_while_off': sum(1 for p in pages if p.get('http') is not None)},
               'baseline_height': baseline, 'clocks': dict(HOST_CLOCKS), 'injected_at': None, 'cleared_at': None,
               'height_at_clear': None, 'recovered_at': None, 'recovered_height': None, 'reader_gap_seconds': None}
        if off_at is not None:
            row['injected_at'] = iso_at(off_at)
        if on['status'] == 'DONE' and cleared_at is not None:
            row['cleared_at'] = iso_at(cleared_at)
            row['height_at_clear'] = timeline.last_height(before=cleared_at)
            deadline = cleared_at + recovery_timeout
            while True:
                found = timeline.recovered(cleared_at, row['height_at_clear'], made)
                if found or clock() > deadline:
                    break
                sleep(period)
            if found:
                row['recovered_at'], row['recovered_height'] = found
            end = instant(found[0]).timestamp() if found else clock()
            row['reader_gap_seconds'] = timeline.largest_gap(cleared_at, end, answered_only=False)
    say(f'== {name}: {"caught back up at " + str(row["recovered_height"]) if row["recovered_at"] else "no recovery"}')
    return row


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
    r.add_argument('--scenarios', default=','.join(spec['name'] for spec in SCENARIOS),
                   help='comma-separated (default: the sentry\'s four; validator_restart on the validator)')
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
        store = staging_founder.open_store(args.store, args.work / 'store-scratch')
        channel = staging_founder.Channel(store, args.chain, args.host, args.work, args.cli, args.home, args.key,
                                          args.paper, args.signer)
        if 'host_loss' in args.scenarios.split(','):
            raise Failed('host_loss needs a power controller: it runs in the CI staging network only, '
                         'the real hosts\' power path (the provider\'s API) is not built yet')
        try:
            scenarios = tuple(BY_NAME[name] for name in args.scenarios.split(','))
        except KeyError as error:
            raise Failed(f'{error} is not a registered fault scenario')
        probe = channel.run('fault-clear')
        if probe['status'] != 'DONE':
            raise Failed(f'the fault commands do not run on this host (tc, nft or the interface): {probe.get("steps")}')
        rows = run_scenarios(channel, lambda: staging_founder.status_page(args.status_url),
                             staging_founder.block_instant, scenarios)
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
