#!/usr/bin/env python3
"""The validator recovery drill (simulation plan, T9): the validator's host
is lost and replaced by the fence-first procedure (validator recovery, F17;
node/docs/operations/validator-recovery.md), and the drill writes
digest-bound evidence that `recovery.py verify` re-evaluates from the file
alone.

  recovery.py verify --evidence FILE --genesis NATIVE_GENESIS

The evidence records the procedure's instants on one clock (declared,
fenced, the sentry's consensus log set aside, installed, the replacement's
signing guard set, the sentry started and seen held past its startup
budget, the validator started, the first block after the halt), the halt height H
read twice from the sentry, the heights both nodes reached after it, their
block and application hashes at one height after it, the validator set the
client's status view lists before and after, and the replacement's
`host_install.py verify`. Each rule of the procedure is one check:

- the order: nothing installed before the old host was fenced, and the
  validator started only after the sentry's old votes were set aside, its
  signing guard was set, and the sentry was started and held;
- the signing guard: the replacement's signing state raised to H before its
  first start (host_install.py guard H), so that its key refuses every
  height at or below H. A replacement that finds no peer and starts a new
  chain from genesis (incident 2026-11-10) then cannot sign its height 1;
- the sentry held: the sentry's engine read at H, never above it (with the
  old validator fenced and the replacement stopped, nothing else can sign)
  and never unanswered, from its start until past its startup budget (its
  service.json: startup, stop and kill bounds, and a margin), before the
  validator starts;
- the halt confirmed: the sentry's height read twice, unchanged;
- signing again: the validator past H + 1 and the sentry following, the same
  block and application hash on both at a height after H;
- no double sign: the same validator set before and after (with one
  validator, a double sign removes it and the chain cannot continue);
- the replacement verifies against its bundle;
- the time from declared to the first block after H within the approved
  validator recovery objective (launch/operations/OBJECTIVES.json,
  recovery.rto_minutes.validator: 8 hours).

Any broken rule is FAIL; a missing reading is INCONCLUSIVE, which no run
counts as a pass. Only PASS exits 0. The target the record was built with is
covered by the digest, and `verify` refuses one that is not the approved
objective.

The lean scope is the CI drill on the staging network (`staging_network.py
validator-recovery-drill`), where the runner's wipe.sh stands for the
provider deleting the old server.

On the real hosts each step is at a console (the provider's for the fence,
the hosts' for the rest; install types the seal code), so the founder marks
each step as it is done and the readings come through the public paths only
(staging_founder.py): the sentry's status page, the release CLI pinned to
its client channel, and reconcile-read through each host's agent. Every
instant is the founder's clock. A run is a directory holding run.json, the
observations so far:

  recovery.py start --run DIR --cli DYTALLIX --client-home DIR
  recovery.py mark --run DIR fenced|wal_set_aside|installed|guarded|sentry_started|validator_started
                   [--moved-to PATH] [--height H]
  recovery.py halt --run DIR --status-url URL
  recovery.py hold --run DIR --service SENTRY_SERVICE.json --store STORE.json --chain ID --sentry LABEL
                   --cli DYTALLIX --home DIR --key KEY.json --paper FILE --signer DYTALLIX_ROOT_SIGN --work DIR
  recovery.py first-block --run DIR --status-url URL [--timeout S]
  recovery.py finish --run DIR --store STORE.json --chain ID --validator LABEL --sentry LABEL
                     --cli DYTALLIX --home DIR --key KEY.json --paper FILE --signer DYTALLIX_ROOT_SIGN
                     --work DIR --client-home DIR --genesis NATIVE_GENESIS --host-verify-exit N
                     --out FILE

start is the declaration: the validator set read through the client
channel, then the instant. Each mark is one console step done, at the
founder's clock, recorded once and as done: a step marked out of the
procedure's order is kept, and the order check fails it. halt reads the
status page twice, a minute apart, after the fence: H. guarded is marked
with the height host_install.py guard printed at the replacement's console.
hold is the start gate: after the sentry starts, it reads the sentry's
engine through its agent (reconcile-read; the status page only serves once
the sentry is ready, and it needs a peer for that) until past the sentry's
startup budget, and says to start the validator only if every reading
answered at H; otherwise it exits 1 and the validator stays stopped. It can
be run again after a failed hold (the failed one is kept). first-block polls
the status page from the validator's start until it serves a height above
H (with one validator, only the replacement can have signed it), stops
with an order to stop the validator if the sentry ever serves a height
below H, and keeps
the reader's longest gap between answers, which can only make the recovery
look slower. finish sends reconcile-read to both hosts' agents for the same
look (the replacement's agent installed again after it started), compares
the highest height both read at least AFTER blocks past H, reads the
validator set again, takes the replacement's `host_install.py verify` exit
as read at its console, and writes the evidence; it exits 0 only on PASS.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
OBJECTIVES = HERE.parents[2] / 'launch' / 'operations' / 'OBJECTIVES.json'
SCHEMA = 'dytallix.validator-recovery.v2'
PASS, FAIL, INCONCLUSIVE = 'PASS', 'FAIL', 'INCONCLUSIVE'
# The procedure's instants, in the order the procedure requires them.
STEPS = ('declared_at', 'fenced_at', 'wal_set_aside_at', 'installed_at', 'guarded_at', 'sentry_started_at',
         'sentry_held_at', 'validator_started_at', 'first_block_after_halt_at')
# The sentry is held past its startup budget: the supervisor's startup, stop
# and kill bounds from its service.json, and this margin, in seconds.
HOLD_MARGIN = 60
HOLD_BOUNDS = ('startup_millis', 'stop_millis', 'kill_millis')
# Heights past H both nodes must reach: signing again, not one block.
AFTER = 3


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def approved_target(path=OBJECTIVES):
    """The approved validator recovery objective, in seconds."""
    return int(json.loads(Path(path).read_bytes())['recovery']['rto_minutes']['validator']) * 60


def hold_budget(process):
    """Seconds the sentry must stay up before the validator starts, from its
    service.json's process bounds."""
    values = [process.get(name) for name in HOLD_BOUNDS]
    if not all(isinstance(v, int) and not isinstance(v, bool) and v > 0 for v in values):
        raise Failed(f'the sentry\'s service.json has no {", ".join(HOLD_BOUNDS)}')
    return sum(values) / 1000 + HOLD_MARGIN


def held(observations, height, pending=False):
    """(status, detail) of the start gate: the sentry's engine read from its
    start to past its budget, every reading answered at H once the engine
    first answered (within its startup bound), all before the validator
    started. While the hold runs (`pending`), a hold not yet past the budget
    is INCONCLUSIVE, not FAIL."""
    hold = observations.get('sentry_hold')
    instants = observations.get('instants') or {}
    if height is None or not isinstance(hold, dict) or not is_number(instants.get('sentry_started_at')):
        return INCONCLUSIVE, 'the sentry was not held before the validator started'
    # The hold counts from its own start: a hold run again after the sentry
    # was started again counts from then, not from the first start.
    started = hold.get('from')
    if not is_number(started) or started < instants['sentry_started_at']:
        return FAIL, 'the hold did not start after the sentry was started'
    readings = hold.get('readings') or []
    try:
        budget = hold_budget(hold.get('process') or {})
    except Failed as error:
        return INCONCLUSIVE, str(error)
    if len(readings) < 2 or not all(isinstance(r, dict) and is_number(r.get('at')) for r in readings):
        return INCONCLUSIVE, 'fewer than two readings of the sentry'
    if readings[0]['at'] < started:
        return FAIL, 'a reading before the sentry was started'
    answered = [i for i, r in enumerate(readings) if isinstance(r.get('height'), int)]
    starting = answered[0] if answered else len(readings)
    if any(r['at'] - started > hold.get('process', {})['startup_millis'] / 1000 for r in readings[:starting]):
        return FAIL, 'the sentry\'s engine did not answer within its startup bound'
    if not answered:
        return (INCONCLUSIVE if pending else FAIL), 'the sentry\'s engine has not answered yet'
    readings = readings[starting:]
    unanswered = [r for r in readings if not isinstance(r.get('height'), int)]
    if unanswered:
        return FAIL, f'the sentry did not answer {unanswered[0]["at"] - started:.0f} s after its start: it was not up'
    above = [r['height'] for r in readings if r['height'] > height]
    if above:
        return FAIL, f'the sentry passed H={height} ({above[0]}) before the validator started: another signer'
    below = [r['height'] for r in readings if r['height'] < height]
    if below:
        return FAIL, f'the sentry read {below[0]}, below H={height}'
    span = readings[-1]['at'] - started
    if span < budget:
        return (INCONCLUSIVE if pending else FAIL), (f'the sentry was held {span:.0f} s, under its {budget:.0f} s '
                                                     'startup budget')
    late = instants.get('validator_started_at')
    if is_number(late) and readings[-1]['at'] > late:
        return FAIL, 'the hold ended after the validator started'
    return PASS, f'the sentry answered at H={height} {len(readings)} times over {span:.0f} s (budget {budget:.0f} s)'


def evaluate(observations, target):
    """The checks, each (name, status, detail), from the observations."""
    rows = []

    def add(name, status, detail):
        rows.append({'check': name, 'status': status, 'detail': detail})

    instants = observations.get('instants') or {}
    missing = [s for s in STEPS if not is_number(instants.get(s))]
    if missing:
        add('order', INCONCLUSIVE, f'no instant recorded for {", ".join(missing)}')
    else:
        late = [f'{a} after {b}' for a, b in zip(STEPS, STEPS[1:]) if instants[a] > instants[b]]
        add('order', FAIL if late else PASS, '; '.join(late) if late else
            'fenced before the install; the old votes set aside and the sentry started before the validator')

    halt = observations.get('halt_readings') or []
    if len(halt) < 2 or not all(isinstance(h, int) for h in halt):
        add('halt', INCONCLUSIVE, 'the sentry\'s height was not read twice after the fence')
    else:
        add('halt', PASS if len(set(halt)) == 1 else FAIL,
            f'the sentry stayed at {halt[0]}' if len(set(halt)) == 1 else f'the sentry moved after the fence: {halt}')
    height = halt[-1] if halt and isinstance(halt[-1], int) else None

    guard = observations.get('signing_guard')
    if height is None or not isinstance(guard, dict) or not isinstance(guard.get('height'), int):
        add('signing_guard', INCONCLUSIVE, 'the replacement\'s signing guard was not recorded')
    else:
        add('signing_guard', PASS if guard['height'] == height else FAIL,
            f'the replacement refuses every height at or below {guard["height"]}' if guard['height'] == height
            else f'the signing guard is at {guard["height"]}, not H={height}')
    add('sentry_held', *held(observations, height))

    reached = observations.get('reached') or {}
    if height is None or not all(isinstance(reached.get(n), int) for n in ('validator', 'sentry')):
        add('signing', INCONCLUSIVE, 'no heights after the halt for both nodes')
    else:
        short = [f'{n} at {reached[n]}' for n in ('validator', 'sentry') if reached[n] < height + AFTER]
        add('signing', FAIL if short else PASS, f'not {AFTER} blocks past H={height}: {", ".join(short)}' if short
            else f'the validator at {reached["validator"]} and the sentry at {reached["sentry"]}, past H={height}')

    compared = observations.get('compared') or {}
    nodes = [compared.get(n) for n in ('validator', 'sentry')]
    if height is None or not all(isinstance(n, dict) and n.get('height') for n in nodes):
        add('state', INCONCLUSIVE, 'the nodes were not compared at a height after the halt')
    elif nodes[0] != nodes[1]:
        add('state', FAIL, f'the nodes differ: validator {nodes[0]}, sentry {nodes[1]}')
    elif nodes[0]['height'] <= height:
        add('state', INCONCLUSIVE, f'compared at {nodes[0]["height"]}, not after H={height}')
    else:
        add('state', PASS, f'the same block and application hash at {nodes[0]["height"]}')

    before, after = observations.get('validators_before'), observations.get('validators_after')
    if not before or after is None:
        add('validator_set', INCONCLUSIVE, 'the validator set was not read before and after')
    else:
        add('validator_set', PASS if before == after else FAIL,
            f'{len(before)} validator(s), unchanged' if before == after else
            f'the validator set changed: {before} before, {after} after')

    verified = observations.get('host_verify_exit')
    add('host_verify', INCONCLUSIVE if verified is None else PASS if verified == 0 else FAIL,
        'not run' if verified is None else f'host_install.py verify exited {verified}')

    if missing:
        add('objective', INCONCLUSIVE, 'the recovery time needs every instant')
    else:
        took = instants['first_block_after_halt_at'] - instants['declared_at']
        add('objective', PASS if took <= target else FAIL,
            f'{took:.0f} s from declared to the first block after the halt; the objective is {target} s')
    found = {r['status'] for r in rows}
    status = FAIL if FAIL in found else INCONCLUSIVE if INCONCLUSIVE in found else PASS
    return rows, status


def build(chain_id, genesis_bytes, observations, target):
    rows, status = evaluate(observations, target)
    body = {'schema': SCHEMA, 'chain_id': chain_id, 'genesis': {'native_genesis_sha256': digest(genesis_bytes)},
            'target_seconds': target, 'observations': observations, 'checks': rows, 'status': status}
    return dict(body, sha256=digest(canonical(body)))


def verify(record, genesis_bytes, target=None):
    """Re-evaluate a record from its observations; any edit, another genesis
    or a target that is not the approved objective shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if body['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    target = approved_target() if target is None else target
    if body['target_seconds'] != target:
        raise Failed(f'the record was judged against {body["target_seconds"]} s, not the approved {target} s')
    rows, status = evaluate(body['observations'], target)
    if (rows, status) != (body['checks'], body['status']):
        raise Failed('the recorded checks do not follow from the recorded observations')
    return status


# The real-host run, from the founder's machine.

RUN = 'run.json'
RUN_SCHEMA = 'dytallix.validator-recovery-run.v2'
CLOCK = "the founder's machine"
# The steps done at a console, in the procedure's order.
MARKS = ('fenced', 'wal_set_aside', 'installed', 'guarded', 'sentry_started', 'validator_started')
# The runbook reads the sentry's height twice, a minute apart.
HALT_GAP = 60
# Catch-up from genesis is most of the 8-hour objective.
FIRST_BLOCK_TIMEOUT = 8 * 3600


def load_run(directory):
    path = Path(directory) / RUN
    try:
        run = json.loads(path.read_bytes())
    except FileNotFoundError:
        raise Failed(f'no run at {directory}: start one first') from None
    if run.get('schema') != RUN_SCHEMA:
        raise Failed(f'{path} is not a validator recovery run')
    return run


def save_run(directory, run):
    path = Path(directory) / RUN
    partial = path.with_name(RUN + '.partial')
    partial.write_text(json.dumps(run, indent=2) + '\n')
    os.replace(partial, path)


def validator_set(client):
    found = client.status().get('effective_validators')
    if not found:
        raise Failed('the client\'s status view lists no validator')
    return found


def start(directory, client, clock=time.time):
    """Declared: the validator set read through the client channel before
    anything is fenced, then the instant."""
    directory = Path(directory)
    if (directory / RUN).exists():
        raise Failed(f'a run is already at {directory}')
    before = validator_set(client)
    directory.mkdir(parents=True, exist_ok=True)
    run = {'schema': RUN_SCHEMA,
           'observations': {'clock': CLOCK, 'instants': {'declared_at': clock()}, 'validators_before': before}}
    save_run(directory, run)
    return run


def mark(directory, step, clock=time.time, moved_to=None, height=None):
    """One console step done, at the founder's clock, recorded once and as
    done: a step out of the procedure's order is kept for the order check to
    fail, never refused. guarded takes the height host_install.py guard
    printed."""
    if step not in MARKS:
        raise Failed(f'{step} is not a step of the procedure ({", ".join(MARKS)})')
    if (step == 'guarded') != (height is not None):
        raise Failed('guarded, and only guarded, takes --height: the height host_install.py guard printed')
    run = load_run(directory)
    obs = run['observations']
    if f'{step}_at' in obs['instants']:
        raise Failed(f'{step} is already marked')
    obs['instants'][f'{step}_at'] = clock()
    if step == 'wal_set_aside' and moved_to:
        obs['wal_set_aside'] = {'host': 'sentry', 'moved_to': moved_to}
    if step == 'guarded':
        obs['signing_guard'] = {'height': height}
    save_run(directory, run)
    return obs['instants'][f'{step}_at']


def page_height(page):
    height = page.get('height')
    return height if isinstance(height, int) and not isinstance(height, bool) else None


def halt(directory, read_page, sleep=time.sleep, gap=HALT_GAP):
    """The halt height H: the sentry's status page read twice, `gap` seconds
    apart, after the fence. A page without a height is kept as no reading,
    and the pair is read again (the earlier pages kept) until both have a
    height, while the old votes are not yet set aside."""
    run = load_run(directory)
    obs = run['observations']
    if 'fenced_at' not in obs['instants']:
        raise Failed('the halt is read after the fence: mark it first')
    readings = obs.get('halt_readings')
    if readings and None not in readings:
        raise Failed('the halt is already read')
    if readings and 'wal_set_aside_at' in obs['instants']:
        raise Failed('the old votes are already set aside: the halt can no longer be read')
    if readings:
        obs.setdefault('halt_unread', []).append(obs.pop('halt_pages'))
    pages = [read_page()]
    sleep(gap)
    pages.append(read_page())
    obs['halt_readings'] = [page_height(p) for p in pages]
    obs['halt_pages'] = pages
    save_run(directory, run)
    return obs['halt_readings']


def hold(directory, read_height, process, clock=time.time, sleep=time.sleep, period=15, say=print):
    """The start gate: after the sentry starts and before the validator does,
    the sentry's engine read every `period` seconds until past its startup
    budget. Once its engine first answers (within its startup bound), every
    reading must answer at H (above H, something else signs). The first miss
    ends the hold, FAIL: the validator stays stopped. Run again, a new hold
    replaces a failed one, which is kept."""
    run = load_run(directory)
    obs = run['observations']
    instants = obs['instants']
    for step, why in (('sentry_started_at', 'the sentry is held after it starts: mark it first'),
                      ('guarded_at', 'set the signing guard first: host_install.py guard H, then mark guarded')):
        if step not in instants:
            raise Failed(why)
    if 'validator_started_at' in instants:
        raise Failed('the validator is already started: the hold was before it')
    if 'sentry_held_at' in instants:
        raise Failed('the sentry is already held')
    readings = obs.get('halt_readings') or []
    if len(readings) < 2 or readings[-1] is None:
        raise Failed('no halt height: read the halt first')
    height = readings[-1]
    guard = (obs.get('signing_guard') or {}).get('height')
    if guard != height:
        raise Failed(f'the signing guard is at {guard}, not H = {height}: set it again at H')
    budget = hold_budget(process)
    if obs.get('sentry_hold'):
        obs.setdefault('sentry_hold_failed', []).append(obs.pop('sentry_hold'))
    record = {'process': {name: process[name] for name in HOLD_BOUNDS}, 'from': clock(), 'readings': []}
    obs['sentry_hold'] = record
    while True:
        # The instant the read is sent: the engine cannot have been read
        # before it, and an agent's answer may come minutes later.
        now = clock()
        found = read_height()
        record['readings'].append({'at': now, 'height': found})
        say(f'== the sentry\'s engine read {found} at {now - record["from"]:.0f} s into the hold')
        status, detail = held(obs, height, pending=True)
        if status == FAIL:
            save_run(directory, run)
            raise Failed(f'{detail}. Do not start the validator: find why, start the sentry again, and hold again')
        if status == PASS:
            instants['sentry_held_at'] = now
            save_run(directory, run)
            return detail
        save_run(directory, run)
        sleep(period)


def first_block(directory, read_page, clock=time.time, sleep=time.sleep, timeout=FIRST_BLOCK_TIMEOUT, period=1):
    """The first block after the halt, at the founder's clock: the status page
    polled until it serves a height above H. With one validator only the
    replacement can have signed it. The reader's longest gap between answers
    (a status, or a refused connection) is kept: a gap can only make the
    recovery look slower."""
    run = load_run(directory)
    obs = run['observations']
    if 'validator_started_at' not in obs['instants']:
        raise Failed('the first block is awaited after the validator starts: mark it first')
    if 'first_block_after_halt_at' in obs['instants']:
        raise Failed('the first block after the halt is already read')
    readings = obs.get('halt_readings') or []
    if len(readings) < 2 or readings[-1] is None:
        raise Failed('no halt height: read the halt first')
    height = readings[-1]
    deadline = last = clock()
    deadline += timeout
    gap = 0.0
    while clock() < deadline:
        page = read_page()
        now = clock()
        if page.get('http') is not None or page.get('refused'):
            gap, last = max(gap, now - last), now
        if page_height(page) is not None and page_height(page) < height:
            save_run(directory, run)
            raise Failed(f'the sentry serves {page["height"]}, below H = {height}: stop the validator at once')
        if (page_height(page) or 0) > height:
            obs['instants']['first_block_after_halt_at'] = now
            obs['first_block'] = {'height': page['height'], 'block_time': page.get('time'),
                                  'read_at': page.get('read_at'), 'reader_gap_seconds': round(gap, 1)}
            save_run(directory, run)
            return page['height']
        sleep(period)
    raise Failed(f'the sentry did not pass H = {height} within {timeout} s')


def engine_readings(result):
    """{height: {block_hash, app_hash}} from a reconcile-read result: each
    operator status that answered."""
    import reconcile
    found = {}
    if result.get('status') not in ('DONE', 'FAILED'):
        return found
    for step in result.get('steps') or []:
        if (step.get('argv') or [])[-1:] != ['status'] or step.get('exit') != 0:
            continue
        try:
            height, block, app = reconcile.engine_status(step.get('stdout') or '')
        except reconcile.Missing:
            continue
        found[height] = {'block_hash': block, 'app_hash': app}
    return found


def agent_height(channel, timeout=900):
    """The engine's height as one reconcile-read through a host's agent, or
    None if it did not answer."""
    import reconcile
    sequence = channel.send(reconcile.READ_COMMAND)
    return max(engine_readings(channel.wait([sequence], timeout)[0]), default=None)


def finish(directory, channels, client, chain_id, genesis_bytes, host_verify_exit, target, say=print,
           wall=time.time, sleep=time.sleep, timeout=900):
    """Signing again: reconcile-read through the validator's and the sentry's
    agents (`channels`, by role) for the same look, the highest height both
    read that is AFTER blocks past H compared, the validator set read again,
    and the replacement's verify exit as read at its console. Returns the
    evidence; run again, it reads again."""
    import reconcile
    run = load_run(directory)
    obs = run['observations']
    if 'first_block_after_halt_at' not in obs['instants']:
        raise Failed('finish after the first block after the halt')
    height = obs['halt_readings'][-1]
    every = {getattr(channel, 'every', 1) for channel in channels.values()}
    if len(every) > 1:
        raise Failed(f'the hosts look in the store at different intervals ({sorted(every)} minutes)')
    every = every.pop()
    pause = reconcile.send_pause(wall(), every)
    if pause:
        say(f'== waiting {pause:.0f} s to send inside the hosts\' {every}-minute look')
        sleep(pause)
    sent = {role: channel.send(reconcile.READ_COMMAND) for role, channel in channels.items()}
    read, bound = {}, {}
    for role, channel in channels.items():
        result = channel.wait([sent[role]], timeout)[0]
        read[role] = engine_readings(result)
        bound[role] = {'host': channel.host, 'sequence': sent[role], 'status': result.get('status'),
                       'command_sha256': result.get('command_sha256'), 'finished_at': result.get('finished_at'),
                       'result': (getattr(channel, 'names', None) or {}).get(sent[role])}
        say(f'== {role}: {reconcile.READ_COMMAND} {result.get("status")}, {len(read[role])} heights read')
    obs['agent'] = bound
    obs['reached'] = {role: max(found, default=None) for role, found in read.items()}
    both = [h for h in set(read.get('validator') or {}) & set(read.get('sentry') or {}) if h >= height + AFTER]
    obs.pop('compared', None)
    if both:
        at = max(both)
        obs['compared'] = {role: {'height': at, **read[role][at]} for role in ('validator', 'sentry')}
    obs['validators_after'] = validator_set(client)
    obs['host_verify_exit'] = host_verify_exit
    save_run(directory, run)
    return build(chain_id, genesis_bytes, obs, target)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True, help='native-genesis.json')
    s = commands.add_parser('start')
    m = commands.add_parser('mark')
    m.add_argument('step', choices=MARKS)
    m.add_argument('--moved-to', help='where the sentry\'s consensus log was set aside')
    m.add_argument('--height', type=int, help='for guarded: the height host_install.py guard printed')
    h = commands.add_parser('halt')
    g = commands.add_parser('hold')
    g.add_argument('--service', type=Path, required=True, help='the sentry bundle\'s service.json')
    for name in ('store', 'home', 'key', 'paper', 'signer', 'work', 'cli'):
        g.add_argument(f'--{name}', type=Path, required=True)
    for name in ('chain', 'sentry'):
        g.add_argument(f'--{name}', required=True)
    b = commands.add_parser('first-block')
    b.add_argument('--timeout', type=int, default=FIRST_BLOCK_TIMEOUT)
    f = commands.add_parser('finish')
    for name in ('store', 'home', 'key', 'paper', 'signer', 'work', 'genesis', 'out'):
        f.add_argument(f'--{name}', type=Path, required=True)
    for name in ('chain', 'validator', 'sentry'):
        f.add_argument(f'--{name}', required=True)
    f.add_argument('--host-verify-exit', type=int, required=True,
                   help='the replacement\'s host_install.py verify exit, read at its console')
    for p in (s, m, h, g, b, f):
        p.add_argument('--run', type=Path, required=True)
    for p in (s, f):
        p.add_argument('--cli', type=Path, required=True)
        p.add_argument('--client-home', type=Path, required=True)
    for p in (h, b):
        p.add_argument('--status-url', required=True)
    args = parser.parse_args()
    try:
        if args.command == 'verify':
            status = verify(json.loads(args.evidence.read_bytes()), args.genesis.read_bytes())
        else:
            return run_main(args)
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'recovery: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


def run_main(args):
    import reconcile
    import staging_founder
    try:
        return run_step(args, reconcile, staging_founder)
    except (staging_founder.Failed, staging_founder.staging_command.Failed, reconcile.Failed) as error:
        raise Failed(str(error)) from None


def run_step(args, reconcile, staging_founder):
    page = lambda: staging_founder.status_page(args.status_url)  # noqa: E731
    client = lambda: reconcile.Client(args.cli, dict(os.environ, HOME=str(args.client_home)), args.run)  # noqa: E731
    if args.command == 'start':
        start(args.run, client())
        print(f'== declared; the validator set read: {RUN} at {args.run}')
    elif args.command == 'mark':
        mark(args.run, args.step, moved_to=args.moved_to, height=args.height)
        print(f'== {args.step} marked')
    elif args.command == 'hold':
        store = staging_founder.open_store(args.store, args.work / 'store-scratch')
        channel = staging_founder.Channel(store, args.chain, args.sentry, args.work / 'results' / args.sentry,
                                          args.cli, args.home, args.key, args.paper, args.signer)
        process = json.loads(args.service.read_bytes())['process']
        print(f'== {hold(args.run, lambda: agent_height(channel), process)}')
        print('== GO: start the validator now (systemctl start dytallix-node), then mark validator_started')
    elif args.command == 'halt':
        print(f'== the sentry read {halt(args.run, page)} a minute apart')
    elif args.command == 'first-block':
        print(f'== the sentry served height {first_block(args.run, page, timeout=args.timeout)}, past the halt')
    else:
        store = staging_founder.open_store(args.store, args.work / 'store-scratch')
        channels = {role: staging_founder.Channel(store, args.chain, label, args.work / 'results' / label,
                                                  args.cli, args.home, args.key, args.paper, args.signer)
                    for role, label in (('validator', args.validator), ('sentry', args.sentry))}
        record = finish(args.run, channels, client(), args.chain, args.genesis.read_bytes(),
                        args.host_verify_exit, approved_target())
        args.out.write_text(json.dumps(record, indent=2) + '\n')
        for row in record['checks']:
            print(f'{row["status"]:<12} {row["check"]}: {row["detail"]}')
        status = verify(json.loads(args.out.read_bytes()), args.genesis.read_bytes())
        print(f'{status} (evidence sha256 {record["sha256"]})')
        return 0 if status == PASS else 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
