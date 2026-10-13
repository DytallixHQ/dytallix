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

The lean scope is the network and process faults driven through the agent:
latency and loss (`tc netem`) and a peer partition (an `inet dytallix_fault`
nftables table, scoped to the p2p port so the agent's own command path stays
reachable), and a process kill. Peer drops and host loss follow (they need a VM
restart path). The validator keeps producing throughout; recovery is the
sentry catching back up, read through the guest agent, whose virtio serial
survives the fault that the network path does not.
"""
import argparse
from datetime import datetime
import hashlib
import json
from pathlib import Path
import sys

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
        # The sentry did not advance while cut off from its only peer.
        return effect.get('frozen') is True and is_int(effect.get('height'))
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
    args = parser.parse_args()
    try:
        record = json.loads(args.evidence.read_bytes())
        status = verify(record, args.genesis.read_bytes(), args.engine_genesis.read_bytes())
    except (Failed, reconcile.Failed, OSError, KeyError, ValueError) as error:
        print(f'faults: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
