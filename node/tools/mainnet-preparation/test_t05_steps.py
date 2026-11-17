#!/usr/bin/env python3
"""Tests for t05_steps.py: per-step counts and the PASS / LOWER_BOUND / FAIL verdict."""
from datetime import datetime, timezone
import json
import unittest

import t05_steps as t

START = 1_800_000_000.0
STEPS = [[12.0, 600.0], [24.0, 600.0], [36.0, 2400.0], [48.0, 600.0]]


def iso(seconds):
    return datetime.fromtimestamp(seconds, timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')


def run(launch_scale=1.0, commit_scale=1.0, lose_in_hold=0):
    """A synthetic stepped run: one block every 5 s, each step launching
    rate*launch_scale a second, committed in the next block."""
    rows, block_times, offset, lost = [], {}, 0.0, lose_in_hold
    height = 100
    for rate, seconds in STEPS:
        for k in range(int(seconds / 5)):
            at = START + offset + 5 * k
            height += 1
            block_times[str(height)] = iso(at + 5)
            sent = int(rate * 5 * launch_scale)
            committed = int(sent * commit_scale)
            for j in range(sent):
                status = 'committed' if j < committed else 'lost'
                if status == 'committed' and lost and rate == 36.0 and k > 200:
                    status, lost = 'lost', lost - 1
                rows.append({'phase': 'run', 'kind': 'send_drt', 'status': status, 'submitted_at': at + 0.1,
                             'submit_height': height - 1, 'block_height': height if status == 'committed' else None})
        offset += seconds
    evidence = {'chain_id': 'dytallix-staging-3', 'sha256': 'x', 'settings': {'steps': STEPS},
                'block_times': block_times, 'window': {'workers': [{'window': {'started': iso(START)}}],
                                                       'sign_ms': {'n': 1}}}
    return evidence, rows


class Verdict(unittest.TestCase):
    def test_a_held_target_with_nothing_lost_passes(self):
        result = t.summarize(*run())
        self.assertEqual(result['verdict'], 'PASS', result['hold'])
        self.assertEqual([s['asked_per_block'] for s in result['steps']], [60, 120, 180, 240])
        self.assertEqual(result['hold']['seconds'], 1800.0)
        self.assertAlmostEqual(result['steps'][2]['committed_per_block'], 180.0, delta=0.5)

    def test_a_source_that_cannot_launch_the_rate_is_a_lower_bound(self):
        result = t.summarize(*run(launch_scale=0.5))
        self.assertEqual(result['verdict'], 'LOWER_BOUND')
        self.assertTrue(all(s['source_limited'] for s in result['steps']))
        self.assertAlmostEqual(result['highest_committed_per_block'], 120.0, delta=0.5)

    def test_a_loss_in_the_hold_or_a_short_chain_fails(self):
        self.assertEqual(t.summarize(*run(lose_in_hold=1))['verdict'], 'FAIL')
        self.assertEqual(t.summarize(*run(commit_scale=0.9))['verdict'], 'FAIL')

    def test_a_run_without_steps_is_refused(self):
        evidence, rows = run()
        del evidence['settings']['steps']
        with self.assertRaisesRegex(t.Failed, 'no rate steps'):
            t.summarize(evidence, rows)


class RealEvidence(unittest.TestCase):
    """The drive's evidence as load.evidence writes it: block times are header times (RFC 3339 text)."""

    def test_a_load_record_is_read_as_the_drive_writes_it(self):
        import load
        import test_load
        rows, times = [], {}
        for k in range(12):
            height = 101 + k
            times[str(height)] = datetime.fromtimestamp(START + 5 * k + 5, timezone.utc).strftime(
                '%Y-%m-%dT%H:%M:%S') + '.123456789Z'
            for _ in range(10):
                row = test_load.tx_row('run', 'send_drt', block=height, submit=height - 1)
                row['submitted_at'] = START + 5 * k + 0.1
                rows.append(row)
        raw = load.rows_jsonl(rows)
        ledger = load.ledger(rows, 100, 10)
        charged = sum(int(e['charge_udrt']) for e in ledger['entries'])
        settings = dict(test_load.SETTINGS, max_refused=None, steps=[[2.0, 60.0]])
        stamp = lambda s: datetime.fromtimestamp(s, timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        record = load.evidence(test_load.CHAIN, settings, {}, raw, ledger, test_load.supply(0, 100),
                               test_load.supply(charged, 113), times, {},
                               {'workers': [{'window': {'started': iso(START)}}]}, stamp(START), stamp(START + 70))
        record = json.loads(json.dumps(record))
        load.verify(record, raw)
        result = t.summarize(record, [json.loads(line) for line in raw.splitlines()],
                             target_per_block=10, hold_seconds=60)
        self.assertEqual((result['verdict'], result['hold']['committed_per_block'], result['hold']['blocks']),
                         ('PASS', 10.0, 11))

    def test_a_block_time_that_is_not_a_header_time_is_refused(self):
        evidence, rows = run()
        evidence['block_times']['101'] = 1_800_000_005.0
        with self.assertRaisesRegex(t.Failed, 'not a header time'):
            t.summarize(evidence, rows)


if __name__ == '__main__':
    unittest.main()
