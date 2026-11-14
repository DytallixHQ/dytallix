#!/usr/bin/env python3
"""T05 capacity verdict from a stepped load run (load_workers.py drive
--steps; P01_T06_R03_T05_WITHIN_HETZNER_LIMITS_2026-11-13 and
P01_T06_T05_CAPACITY_RUN_2026-11-10).

  t05_steps.py --evidence RUN/evidence.json [--rows RUN/transactions.jsonl]
               [--target-per-block 180] [--hold-seconds 1800] [--out FILE]

For each rate step it reports what the load source launched, what the chain
committed (per second and per block, by the blocks whose header time falls in
the step), what was lost or refused, and the inclusion delay in blocks. The
verdict reads the last --hold-seconds of the steps at or above the target:

  PASS         at least the target committed per block on average over the
               hold, and no transaction submitted in it lost or refused;
  LOWER_BOUND  the load source launched under 95% of the asked rate in the
               hold (it, not the chain, was the limit): the result is the
               highest committed per block reached, not a pass;
  FAIL         otherwise.

Memory and CPU come from the hosts' monitor history, not from here
(P01_T06_T05_RESOURCE_READING_2026-11-13). Standard library only.
"""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import sys

SCHEMA = 'dytallix.t05-steps.v1'
BLOCK_SECONDS = 5.0
LOST = ('lost', 'refused', 'uncertain', 'unsigned', 'expired')
SOURCE_SHORTFALL = 0.95


class Failed(Exception):
    pass


def epoch(text):
    return datetime.strptime(text, '%Y-%m-%dT%H:%M:%S.%fZ').replace(tzinfo=timezone.utc).timestamp()


def percentile(values, q):
    if not values:
        return None
    values = sorted(values)
    return values[min(len(values) - 1, int(round(q * (len(values) - 1))))]


def window_start(evidence):
    workers = evidence.get('window', {}).get('workers') or []
    starts = [epoch(w['window']['started']) for w in workers if w.get('window', {}).get('started')]
    if not starts:
        raise Failed('the evidence names no worker window start')
    return min(starts)


def interval(rows, block_times, lo, hi):
    """What was submitted in [lo, hi) and what committed in blocks timed in it."""
    seconds = hi - lo
    sent = [r for r in rows if r['submitted_at'] is not None and lo <= r['submitted_at'] < hi]
    blocks = sorted(h for h, t in block_times.items() if lo <= t < hi)
    in_blocks = set(blocks)
    committed = [r for r in rows if r['status'] == 'committed' and r['block_height'] in in_blocks]
    lost = [r for r in sent if r['status'] in LOST]
    delays = [r['block_height'] - r['submit_height'] for r in sent
              if r['status'] == 'committed' and r['submit_height'] is not None]
    return {'seconds': round(seconds, 1), 'launched': len(sent), 'launched_per_s': round(len(sent) / seconds, 2),
            'blocks': len(blocks), 'committed': len(committed),
            'committed_per_s': round(len(committed) / seconds, 2),
            'committed_per_block': round(len(committed) / len(blocks), 1) if blocks else None,
            'lost': len(lost), 'lost_statuses': sorted({r['status'] for r in lost}),
            'delay_blocks_p50': percentile(delays, 0.50), 'delay_blocks_p95': percentile(delays, 0.95),
            'kinds': sorted({r['kind'] for r in sent})}


def summarize(evidence, rows, target_per_block=180, hold_seconds=1800.0, block_seconds=BLOCK_SECONDS):
    steps = evidence.get('settings', {}).get('steps')
    if not steps:
        raise Failed('the run has no rate steps (load_workers.py drive --steps)')
    rows = [r for r in rows if r.get('phase') == 'run']
    block_times = {int(h): float(t) for h, t in (evidence.get('block_times') or {}).items()}
    if not block_times:
        raise Failed('the evidence has no block times')
    start = window_start(evidence)
    out, offset = [], 0.0
    for rate, seconds in steps:
        row = interval(rows, block_times, start + offset, start + offset + seconds)
        row.update(asked_per_s=rate, asked_per_block=round(rate * block_seconds, 1),
                   source_limited=row['launched_per_s'] < SOURCE_SHORTFALL * rate)
        out.append(row)
        offset += seconds
    # The hold: the last hold_seconds of the last run of steps at or above the target.
    at_target = [i for i, (rate, _) in enumerate(steps) if rate * block_seconds >= target_per_block]
    if not at_target:
        raise Failed(f'no step asks for {target_per_block} a block')
    first = last = at_target[0]
    while last + 1 < len(steps) and steps[last + 1][0] == steps[first][0]:
        last += 1
    hold_end = start + sum(s for _, s in steps[:last + 1])
    hold_start = max(start + sum(s for _, s in steps[:first]), hold_end - hold_seconds)
    hold = interval(rows, block_times, hold_start, hold_end)
    asked = steps[first][0]
    hold.update(asked_per_s=asked, source_limited=hold['launched_per_s'] < SOURCE_SHORTFALL * asked)
    best = max((s['committed_per_block'] or 0) for s in out)
    if hold['source_limited']:
        verdict = 'LOWER_BOUND'
    elif (hold['committed_per_block'] or 0) >= target_per_block and hold['lost'] == 0 \
            and hold_end - hold_start >= hold_seconds:
        verdict = 'PASS'
    else:
        verdict = 'FAIL'
    return {'schema': SCHEMA, 'chain_id': evidence.get('chain_id'), 'evidence_sha256': evidence.get('sha256'),
            'target_per_block': target_per_block, 'hold_seconds': hold_seconds, 'block_seconds': block_seconds,
            'steps': out, 'hold': hold, 'highest_committed_per_block': best,
            'sign_ms': evidence.get('window', {}).get('sign_ms'), 'verdict': verdict}


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument('--evidence', type=Path, required=True)
    p.add_argument('--rows', type=Path)
    p.add_argument('--target-per-block', type=int, default=180)
    p.add_argument('--hold-seconds', type=float, default=1800.0)
    p.add_argument('--out', type=Path)
    a = p.parse_args()
    try:
        evidence = json.loads(a.evidence.read_bytes())
        rows_path = a.rows or a.evidence.with_name('transactions.jsonl')
        rows = [json.loads(line) for line in rows_path.read_text().splitlines() if line.strip()]
        result = summarize(evidence, rows, a.target_per_block, a.hold_seconds)
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f't05_steps: {error}', file=sys.stderr)
        return 1
    text = json.dumps(result, indent=2) + '\n'
    if a.out:
        a.out.write_text(text)
    for s in result['steps']:
        print(f'{s["asked_per_block"]:>6}/block asked: launched {s["launched_per_s"]}/s, committed '
              f'{s["committed_per_block"]}/block, lost {s["lost"]}, delay p95 {s["delay_blocks_p95"]} blocks'
              + (' (source-limited)' if s['source_limited'] else ''))
    print(f'hold: {result["hold"]["committed_per_block"]}/block over {result["hold"]["seconds"]} s, '
          f'lost {result["hold"]["lost"]} -> {result["verdict"]}')
    return 0 if result['verdict'] == 'PASS' else 1


if __name__ == '__main__':
    raise SystemExit(main())
