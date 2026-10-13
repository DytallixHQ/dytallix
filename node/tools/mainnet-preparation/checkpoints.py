#!/usr/bin/env python3
"""The checkpoint recorder (simulation plan, T7): at each of a run's
checkpoints it runs that checkpoint's checks and writes digest-bound evidence
that `checkpoints.py verify` re-evaluates from the file alone.

  checkpoints.py verify --evidence FILE --genesis NATIVE_GENESIS \
      --engine-genesis ENGINE_GENESIS [--qualifying R07]

R07's checkpoints and their verbatim checks are the brief's, kept in
[launch-rehearsals.json](../../docs/mainnet/launch-rehearsals.json)
(`final_dress_rehearsal_checkpoints`); `schedule('R07')` builds them with the
offsets below, and only that schedule's digest counts as the dress rehearsal.
R01 to R06 reconcile "at each checkpoint" but the brief sets no schedule for
them; that is an open P01 decision, added later as data, so this tool takes a
schedule as input and never invents one.

A checkpoint's status is PASS only when every check is PASS; a FAIL anywhere
is a FAIL that stops the run (the plan's integrity rule); otherwise, with a
check left INCONCLUSIVE, the checkpoint is INCONCLUSIVE, which no run may
count as a pass. A run record with fewer checkpoints than its schedule is in
progress: INCONCLUSIVE, never a pass.

Each check is re-derived, never trusted from the record. The reconciliation
(T6) and load (T3) checks bind that tool's evidence by digest and re-run its
own `verify`; the direct readings (RPC, wallets, validator participation, the
genesis and binary digests) keep their raw observation, from which the verdict
follows. A check whose source is not built yet is INCONCLUSIVE and names the
tool it waits for (the real-host harness T2, the fault and recovery drills T5
and T9, the operator's review). Genesis time comes from the engine genesis, so
moving genesis earlier cannot make a compressed checkpoint look on time;
bound evidence whose own finish time falls outside a checkpoint's window does
not count for it.
"""
import argparse
from datetime import datetime, timedelta
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import load as load_tool  # noqa: E402
import reconcile  # noqa: E402

SCHEMA = 'dytallix.checkpoint-run.v1'
PASS, FAIL, INCONCLUSIVE = reconcile.PASS, reconcile.FAIL, reconcile.INCONCLUSIVE
REGISTER = HERE.parent.parent / 'docs/mainnet/launch-rehearsals.json'

# The R07 offsets from genesis, in seconds (launch-rehearsals.md, "R07's
# checkpoints"): the label is the brief's, the offset the tool's.
OFFSETS = {
    'T MINUS 6 HOURS': -6 * 3600, 'T MINUS 1 HOUR': -3600, 'T ZERO': 0,
    'T PLUS 15 MINUTES': 15 * 60, 'T PLUS 1 HOUR': 3600,
    'T PLUS 6 HOURS': 6 * 3600, 'T PLUS 24 HOURS': 24 * 3600,
}


class Failed(Exception):
    pass


def digest(data):
    return hashlib.sha256(data).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':')).encode()


def instant(text):
    return datetime.fromisoformat(text.replace('Z', '+00:00'))


# The check table: every verbatim check string the brief lists, mapped to the
# category that evaluates it. `verify` recomputes the category from the string,
# so a record cannot file a check under a softer category. The set of keys must
# equal the JSON's strings (test_checkpoints covers this).
CHECK_TABLE = {
    # Genesis allocation, checked before the chain runs (T-6h).
    'Verify DRT supply.': 'genesis_supply',
    'Verify DGT supply.': 'genesis_supply',
    # Supply reconciliation and cross-node state, once the chain runs (bind T6).
    'Verify token supply.': 'supply',
    'Perform full supply reconciliation.': 'supply',
    'Verify state consistency across validators.': 'state',
    'Perform final state consistency review.': 'state',
    # Consensus and economic activity (bind the T3 load run).
    'Confirm consensus.': 'liveness',
    'Confirm block production.': 'liveness',
    'Execute canonical PQC transaction.': 'transactions',
    'Execute DRT transaction.': 'transactions',
    'Execute DGT transaction.': 'transactions',
    'Create delegation.': 'transactions',
    'Verify fee accounting.': 'transactions',
    'Verify finalization.': 'transactions',
    # Direct readings through the client channel and the pinned digests.
    'Verify expected validator participation.': 'participation',
    'Perform full validator review.': 'participation',
    'Verify RPC.': 'rpc',
    'Verify RPC infrastructure.': 'rpc',
    'Verify wallet connectivity.': 'wallet',
    'Verify wallets.': 'wallet',
    'Verify genesis.': 'genesis_digest',
    'Confirm genesis hash.': 'genesis_digest',
    'Verify binaries.': 'binary_digest',
    'Verify checksums.': 'binary_digest',
    'Verify release commit.': 'binary_digest',
    'Confirm binary hash.': 'binary_digest',
    # Not sourced here: each names the tool or review it waits for.
    'Confirm validator coordination.': 'real_host',
    'Verify validators.': 'real_host',
    'Verify validator keys.': 'real_host',
    'Verify infrastructure.': 'real_host',
    'Verify networking.': 'real_host',
    'Verify DNS.': 'real_host',
    'Start validators.': 'real_host',
    'Confirm peers.': 'real_host',
    'Confirm system clocks.': 'real_host',
    'Confirm validator keys.': 'real_host',
    'Confirm configuration.': 'real_host',
    'Confirm network parameters.': 'real_host',
    'Verify all genesis allocations.': 'allocations',
    'Confirm token allocations.': 'allocations',
    'Verify monitoring.': 'monitoring',
    'Verify alerts.': 'monitoring',
    'Verify backups.': 'monitoring',
    'Verify recovery procedures.': 'monitoring',
    'Verify staking.': 'staking_review',
    'Verify rewards.': 'staking_review',
    'Review telemetry.': 'review',
    'Review logs.': 'review',
    'Review validator participation.': 'review',
    'Review token accounting.': 'review',
    'Review transaction statistics.': 'review',
    'Review security alerts.': 'review',
    'Review staking activity.': 'review',
}

# The unsourced categories and the tool or review each waits for.
UNSOURCED = {
    'real_host': 'the real-host harness (T2)',
    'allocations': 'the rehearsal-genesis allocation mapping (T1)',
    'monitoring': 'the monitoring, backup and recovery review (the monitor drill, T9, the operator)',
    'staking_review': 'the staking and rewards review (the operator, the statistical campaign)',
    'review': 'the operator review of telemetry, logs and alerts',
}


def category(check):
    try:
        return CHECK_TABLE[check]
    except KeyError:
        raise Failed(f'{check!r} is not a registered checkpoint check')


# Re-verification of bound sub-evidence. The record keeps a relative path, the
# tool, and the digest(s) it was taken from; `resolve` re-reads, re-hashes and
# re-runs that tool's own verify, so an edited or swapped file shows.

def resolve_bind(bind, base, genesis_bytes):
    """Returns (status, finished, record) for a bound reconciliation or load
    run, or raises Failed if the file is not the one the record names."""
    path = base / bind['evidence']
    data = path.read_bytes()
    if digest(data) != bind['evidence_sha256']:
        raise Failed(f'{bind["evidence"]} is not the evidence the record names')
    record = json.loads(data)
    if bind['tool'] == 'reconcile':
        status = reconcile.verify(record, genesis_bytes)
    elif bind['tool'] == 'load':
        rows = (base / bind['rows']).read_bytes()
        if digest(rows) != bind['rows_sha256']:
            raise Failed(f'{bind["rows"]} is not the transactions file the record names')
        status = load_tool.verify(record, rows)
    else:
        raise Failed(f'{bind["tool"]} is not a bound tool')
    return status, record.get('finished'), record


def in_window(moment, window):
    start, end = window
    at = instant(moment)
    return instant(start) <= at and (end is None or at < instant(end))


def evaluate(check, row, base, window, genesis_sha256, genesis_bytes):
    """The status and detail of one check, derived only from its recorded
    reading or bound evidence. Never reads the recorded status or category."""
    kind = category(check)
    if kind in UNSOURCED:
        return INCONCLUSIVE, f'awaits {UNSOURCED[kind]}'
    bind, inputs = row.get('bind'), row.get('inputs') or {}
    if kind == 'genesis_supply':
        # The genesis allocation, re-derived from the native genesis. There is
        # no approved total to check it against yet (allocation_policy's
        # initial_drt_supply is null, custody unresolved), so it is INCONCLUSIVE.
        totals = reconcile.genesis_totals(json.loads(genesis_bytes))
        return INCONCLUSIVE, (f'genesis allocates {totals["udrt"]} udrt and {totals["udgt"]} udgt; '
                              'awaits the approved genesis supply total (E05, unresolved)')
    if kind in ('supply', 'state', 'liveness', 'transactions'):
        if not bind:
            tool = 'reconciliation (T6)' if kind in ('supply', 'state') else 'load run (T3)'
            return INCONCLUSIVE, f'no {tool} evidence bound at this checkpoint'
        status, finished, bound = resolve_bind(bind, base, genesis_bytes)
        if finished is None or not in_window(finished, window):
            return INCONCLUSIVE, f'the bound {bind["tool"]} evidence is not from this checkpoint window'
        if kind in ('supply', 'state'):
            return status, f'the reconciliation is {status}'
        if kind == 'transactions':
            return status, f'the load run is {status}'
        # Liveness: the plan's block-interval thresholds over the load run.
        interval = bound['summary']['block_interval_s']
        ok = interval['n'] > 0 and interval['p50'] <= load_tool.BLOCK_INTERVAL_P50_S \
            and interval['p99'] <= load_tool.BLOCK_INTERVAL_P99_S
        detail = (f'median {interval.get("p50")} s, 99th percentile {interval.get("p99")} s '
                  f'over {interval["n"]} intervals')
        return (PASS if ok else FAIL), detail
    if not inputs:
        return INCONCLUSIVE, 'no reading taken at this checkpoint'
    if kind == 'participation':
        expected, effective = inputs.get('expected') or [], inputs.get('effective') or []
        ok = bool(expected) and set(expected) <= set(effective)
        return (PASS if ok else FAIL), f'{len(effective)} of {len(expected)} expected validators present'
    if kind == 'rpc':
        return (PASS if inputs.get('reachable') else FAIL), f'the client channel height is {inputs.get("height")}'
    if kind == 'wallet':
        return (PASS if inputs.get('reachable') else FAIL), 'the wallet reached the client channel'
    if kind == 'genesis_digest':
        ok = inputs.get('observed') == genesis_sha256
        return (PASS if ok else FAIL), 'the genesis digest matches the native genesis'
    if kind == 'binary_digest':
        observed, manifest = inputs.get('observed') or {}, inputs.get('manifest') or {}
        ok = bool(manifest) and observed == manifest
        return (PASS if ok else FAIL), f'{len(observed)} binary digests against the manifest'
    raise Failed(f'{kind} has no evaluator')


# The plan's integrity criterion is what stops a run: the supply reconciles
# and every node holds the same application hash. A failure of any other check
# (RPC, wallets, participation) fails the run but does not stop the recording.
INTEGRITY = ('genesis_supply', 'supply', 'state')


def checkpoint_status(statuses):
    if FAIL in statuses:
        return FAIL
    if INCONCLUSIVE in statuses or not statuses:
        return INCONCLUSIVE
    return PASS


def window_of(schedule, index, genesis_time):
    """[this checkpoint's planned time, the next one's), half-open; the last
    checkpoint has no upper bound."""
    genesis = instant(genesis_time)
    start = genesis + timedelta(seconds=schedule[index]['offset_seconds'])
    end = None
    if index + 1 < len(schedule):
        end = genesis + timedelta(seconds=schedule[index + 1]['offset_seconds'])
    return (start.isoformat(), end.isoformat() if end else None)


# Building a schedule. Only R07 is defined; its digest is what `--qualifying`
# compares against.

def schedule(run, register=None):
    if run != 'R07':
        raise Failed(f'no checkpoint schedule is defined for {run}')
    data = register if register is not None else json.loads(REGISTER.read_bytes())
    rows = []
    for item in data['final_dress_rehearsal_checkpoints']:
        label = item['checkpoint']
        if label not in OFFSETS:
            raise Failed(f'{label} has no offset')
        rows.append({'label': label, 'offset_seconds': OFFSETS[label],
                     'checks': list(item['checks_verbatim'])})
    return rows


def schedule_sha256(rows):
    return digest(canonical(rows))


# Evidence. One file holds the whole run: the schedule it followed, every
# checkpoint recorded so far, each chained to the one before, and a digest
# over all of it.

def build_checkpoint(schedule_rows, index, actual_time, rows, prev_sha256, genesis_time):
    window = window_of(schedule_rows, index, genesis_time)
    statuses = [row['status'] for row in rows]
    body = {'label': schedule_rows[index]['label'],
            'offset_seconds': schedule_rows[index]['offset_seconds'],
            'window': list(window), 'actual_time': actual_time,
            'status': checkpoint_status(statuses), 'checks': rows, 'prev_sha256': prev_sha256}
    body['sha256'] = digest(canonical(body))
    return body


def record_checkpoint(schedule_rows, index, readings, base, genesis_bytes, engine_genesis_bytes,
                      prev_sha256, actual_time):
    """Build one checkpoint record. `readings` maps a check's text to its raw
    reading, {'inputs': ...} or {'bind': ...}; a check with no reading is left
    for its evaluator to call INCONCLUSIVE. The status of every check comes from
    the same `evaluate` that `verify` re-runs, so the two always agree."""
    genesis_time = json.loads(engine_genesis_bytes)['genesis_time']
    window = window_of(schedule_rows, index, genesis_time)
    genesis_sha256 = digest(genesis_bytes)
    rows = []
    for check in schedule_rows[index]['checks']:
        raw = readings.get(check, {})
        row = {'check': check, 'category': category(check)}
        if 'inputs' in raw:
            row['inputs'] = raw['inputs']
        if 'bind' in raw:
            row['bind'] = raw['bind']
        status, detail = evaluate(check, row, base, window, genesis_sha256, genesis_bytes)
        row['status'], row['detail'] = status, detail
        rows.append(row)
    return build_checkpoint(schedule_rows, index, actual_time, rows, prev_sha256, genesis_time)


def run_status(schedule_rows, checkpoints):
    if len(checkpoints) < len(schedule_rows):
        statuses = [cp['status'] for cp in checkpoints]
        return FAIL if FAIL in statuses else INCONCLUSIVE
    return checkpoint_status([cp['status'] for cp in checkpoints])


def build_run(run, schedule_rows, chain_id, genesis_bytes, engine_genesis_bytes, checkpoints):
    genesis_time = json.loads(engine_genesis_bytes)['genesis_time']
    body = {'schema': SCHEMA, 'run': run, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'genesis_time': genesis_time, 'schedule_sha256': schedule_sha256(schedule_rows),
            'schedule': schedule_rows, 'checkpoints': checkpoints,
            'status': run_status(schedule_rows, checkpoints)}
    body['sha256'] = digest(canonical(body))
    return body


def verify(record, base, genesis_bytes, engine_genesis_bytes, qualifying=None):
    """Re-evaluate a run record from its readings and bound evidence; any edit,
    a swapped genesis, an out-of-order or compressed checkpoint, or a
    qualifying claim the schedule does not support, shows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['genesis']['native_genesis_sha256'] != digest(genesis_bytes):
        raise Failed('the evidence names another native genesis')
    if record['genesis']['engine_genesis_sha256'] != digest(engine_genesis_bytes):
        raise Failed('the evidence names another engine genesis')
    genesis_time = json.loads(engine_genesis_bytes)['genesis_time']
    if record['genesis_time'] != genesis_time:
        raise Failed('the recorded genesis time is not the engine genesis time')
    schedule_rows = record['schedule']
    if record['schedule_sha256'] != schedule_sha256(schedule_rows):
        raise Failed('the schedule digest does not match the recorded schedule')
    if qualifying is not None:
        if record['schedule_sha256'] != schedule_sha256(schedule(qualifying)):
            raise Failed(f'the schedule is not the {qualifying} schedule')
    genesis_sha256 = digest(genesis_bytes)
    checkpoints = record['checkpoints']
    if len(checkpoints) > len(schedule_rows):
        raise Failed('more checkpoints than the schedule')
    prev = ''
    for index, cp in enumerate(checkpoints):
        planned = schedule_rows[index]
        if cp['label'] != planned['label'] or cp['offset_seconds'] != planned['offset_seconds']:
            raise Failed(f'checkpoint {index} is not {planned["label"]} in order')
        if cp['prev_sha256'] != prev:
            raise Failed(f'checkpoint {cp["label"]} is not chained to the one before')
        window = window_of(schedule_rows, index, genesis_time)
        if list(window) != cp['window']:
            raise Failed(f'checkpoint {cp["label"]} records another window')
        if not in_window(cp['actual_time'], window):
            raise Failed(f'checkpoint {cp["label"]} was taken outside its window (a compressed or early run)')
        checks = cp['checks']
        # The checkpoint must record exactly its schedule's checks, in order.
        if [row['check'] for row in checks] != schedule_rows[index]['checks']:
            raise Failed(f'checkpoint {cp["label"]} does not record its schedule\'s checks')
        statuses = []
        integrity_failed = False
        for row in checks:
            status, detail = evaluate(row['check'], row, base, window, genesis_sha256, genesis_bytes)
            if row.get('category') != category(row['check']):
                raise Failed(f'the recorded category for {row["check"]!r} is wrong')
            if (row['status'], row['detail']) != (status, detail):
                raise Failed(f'the recorded check {row["check"]!r} does not follow from its reading')
            statuses.append(status)
            if status == FAIL and category(row['check']) in INTEGRITY:
                integrity_failed = True
        if checkpoint_status(statuses) != cp['status']:
            raise Failed(f'the recorded status of {cp["label"]} does not follow from its checks')
        if integrity_failed and index != len(checkpoints) - 1:
            raise Failed('an integrity failure must stop the run')
        prev = cp['sha256']
    if run_status(schedule_rows, checkpoints) != record['status']:
        raise Failed('the recorded run status does not follow from its checkpoints')
    return record['status']


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify')
    v.add_argument('--evidence', type=Path, required=True)
    v.add_argument('--genesis', type=Path, required=True, help='native-genesis.json')
    v.add_argument('--engine-genesis', type=Path, required=True, help='the engine genesis.json')
    v.add_argument('--qualifying', help='the run whose schedule must match, e.g. R07')
    args = parser.parse_args()
    try:
        record = json.loads(args.evidence.read_bytes())
        status = verify(record, args.evidence.resolve().parent, args.genesis.read_bytes(),
                        args.engine_genesis.read_bytes(), args.qualifying)
    except (Failed, reconcile.Failed, load_tool.Failed, OSError, KeyError, ValueError) as error:
        print(f'checkpoints: {error}', file=sys.stderr)
        return 1
    print(status)
    return 0 if status == PASS else 1


if __name__ == '__main__':
    raise SystemExit(main())
