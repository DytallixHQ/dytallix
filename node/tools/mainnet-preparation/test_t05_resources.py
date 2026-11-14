"""t05_resources.py: memory and CPU per T05 rate step from the monitor history."""
import contextlib
import gzip
import io
import hashlib
import json
import stat
import tempfile
import textwrap
import unittest
from datetime import datetime, timezone
from pathlib import Path

import t05_resources as tr
import t05_steps

START = datetime(2026, 11, 18, 15, 0, tzinfo=timezone.utc).timestamp()
STEPS = [[12, 600], [24, 600], [36, 600], [36, 1800], [48, 600]]


def evidence(start=START, steps=STEPS):
    return {'chain_id': 'dytallix-staging-3', 'sha256': 'e' * 64, 'settings': {'steps': steps},
            'window': {'workers': [{'window': {'started': datetime.fromtimestamp(start, timezone.utc)
                                                           .strftime('%Y-%m-%dT%H:%M:%S.%fZ')}}]}}


def minutes(lo, hi, mem=lambda t: 40.0, cpu=lambda t: 10.0):
    """One monitor line a minute, as host/monitor.py writes them."""
    return [{'t': round(t), 'h': 1, 'mem': mem(t), 'cpu': cpu(t), 'disk_free': 80.0}
            for t in range(int(lo), int(hi), 60)]


def lines(samples):
    return ''.join(json.dumps(s, separators=(',', ':')) + '\n' for s in samples).encode()


class ReadTests(unittest.TestCase):
    def test_each_step_and_the_hold_read_the_minutes_inside_them(self):
        # Memory rises with the step: 30% + 1% per 10 minutes; CPU is 5% per step.
        step_of = lambda t: next(i for i, (lo, hi) in enumerate(
            [(START + sum(s for _, s in STEPS[:i]), START + sum(s for _, s in STEPS[:i + 1]))
             for i in range(len(STEPS))]) if lo <= t < hi)
        samples = minutes(START - 600, START + 4800, mem=lambda t: 30.0 + (t - START) // 600,
                          cpu=lambda t: 5.0 * (step_of(t) + 1) if START <= t < START + 4200 else 1.0)
        result = tr.read(evidence(), {'sentry-1': samples})
        steps = result['hosts']['sentry-1']['steps']
        self.assertEqual([s['samples'] for s in steps], [10, 10, 10, 30, 10])
        self.assertEqual([s['asked_per_block'] for s in steps], [60, 120, 180, 180, 240])
        self.assertEqual(steps[0]['memory_percent'], {'peak': 30.0, 'median': 30.0})
        self.assertEqual(steps[3]['memory_percent'], {'peak': 35.0, 'median': 34.0})
        self.assertEqual([s['cpu_percent']['peak'] for s in steps], [5.0, 10.0, 15.0, 20.0, 25.0])
        hold = result['hosts']['sentry-1']['hold']
        # The hold is the last 1800 s of the 180/block run: the fourth step, 15:30-16:00.
        self.assertEqual((hold['from'], hold['to'], hold['samples']), ('2026-11-18T15:30:00Z', '2026-11-18T16:00:00Z', 30))
        self.assertEqual(hold['cpu_percent'], {'peak': 20.0, 'median': 20.0})
        self.assertEqual(result['gaps'], [])

    def test_the_steps_are_the_intervals_t05_steps_judges(self):
        steps, hold = t05_steps.windows(evidence())
        result = tr.read(evidence(), {'validator-1': minutes(START, START + 4200)})
        self.assertEqual([(r['from'], r['to']) for r in result['hosts']['validator-1']['steps']],
                         [(tr.stamp(lo), tr.stamp(hi)) for _, lo, hi in steps])
        self.assertEqual(result['hosts']['validator-1']['hold']['from'], tr.stamp(hold[1]))

    def test_a_step_with_no_sample_is_a_gap_not_a_zero(self):
        samples = [s for s in minutes(START, START + 4200) if not START + 600 <= s['t'] < START + 1200]
        result = tr.read(evidence(), {'validator-1': samples})
        step = result['hosts']['validator-1']['steps'][1]
        self.assertEqual((step['samples'], step['memory_percent'], step['cpu_percent']), (0, None, None))
        self.assertEqual(result['gaps'], ['validator-1: no sample in the 120/block step from 2026-11-18T15:10:00Z'])

    def test_a_missing_field_is_skipped_not_counted_as_zero(self):
        samples = minutes(START, START + 4200)
        for s in samples[:10]:
            s['cpu'] = None
        step = tr.read(evidence(), {'sentry-1': samples})['hosts']['sentry-1']['steps'][0]
        self.assertEqual((step['samples'], step['cpu_percent'], step['memory_percent']['peak']), (10, None, 40.0))

    def test_a_day_read_twice_counts_once_and_gzip_reads_like_plain(self):
        samples = minutes(START, START + 4200)
        twice = tr.parse_lines(gzip.compress(lines(samples))) + tr.parse_lines(lines(samples))
        result = tr.read(evidence(), {'sentry-1': twice})
        self.assertEqual(result['hosts']['sentry-1']['steps'][3]['samples'], 30)

    def test_the_days_of_a_run_over_midnight(self):
        lo = datetime(2026, 11, 18, 23, 30, tzinfo=timezone.utc).timestamp()
        self.assertEqual(tr.days(lo, lo + 4200), ['2026-11-18', '2026-11-19'])
        self.assertEqual(tr.days(START, START + 4200), ['2026-11-18'])

    def test_a_line_without_a_time_is_refused(self):
        with self.assertRaises(tr.Failed):
            tr.parse_lines(b'{"mem": 40}\n')


class StoreTests(unittest.TestCase):
    """--store: each sealed day checked against its name's digest, then opened."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        # A stand-in for dytallix-root-sign: history-open copies -in to -out.
        self.signer = self.root / 'signer'
        self.signer.write_text(textwrap.dedent('''\
            #!/usr/bin/env python3
            import shutil, sys
            a = sys.argv
            assert a[1] == 'history-open' and '-paper' in a
            shutil.copyfile(a[a.index('-in') + 1], a[a.index('-out') + 1])
            '''))
        self.signer.chmod(self.signer.stat().st_mode | stat.S_IXUSR)

    def tearDown(self):
        self.directory.cleanup()

    def store(self, objects):
        class Store:
            store = {'results_prefix': 'history/'}

            def list(self, prefix, start_after=None):
                return sorted(k for k in objects if k.startswith(prefix))

            def get(self, key):
                return 200, objects[key]
        return Store()

    def sealed(self, host, day, samples, tamper=False):
        raw = gzip.compress(lines(samples))
        digest = hashlib.sha256(raw).hexdigest()
        return f'history/dytallix-staging-3/history/{host}/{day}-{digest}.bin', raw + (b'x' if tamper else b'')

    def test_the_days_wanted_are_fetched_checked_and_opened(self):
        key, raw = self.sealed('sentry-1', '2026-11-18', minutes(START, START + 4200))
        other, other_raw = self.sealed('sentry-1', '2026-11-17', minutes(START - 86400, START - 86000))
        samples = tr.fetch(self.store({key: raw, other: other_raw}), 'dytallix-staging-3', 'sentry-1',
                           ['2026-11-18'], self.signer, self.root / 'paper', self.root / 'work')
        self.assertEqual(len(samples), 70)

    def test_a_copy_that_does_not_match_its_name_is_refused(self):
        key, raw = self.sealed('sentry-1', '2026-11-18', minutes(START, START + 4200), tamper=True)
        with self.assertRaisesRegex(tr.Failed, 'does not match its name'):
            tr.fetch(self.store({key: raw}), 'dytallix-staging-3', 'sentry-1', ['2026-11-18'],
                     self.signer, self.root / 'paper', self.root / 'work')

    def test_a_day_not_yet_uploaded_is_refused(self):
        with self.assertRaisesRegex(tr.Failed, 'no uploaded history for 2026-11-18'):
            tr.fetch(self.store({}), 'dytallix-staging-3', 'sentry-1', ['2026-11-18'],
                     self.signer, self.root / 'paper', self.root / 'work')


class MainTests(unittest.TestCase):
    def test_history_files_in_and_the_record_out(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / 'evidence.json').write_text(json.dumps(evidence()))
            (d / 's.jsonl.gz').write_bytes(gzip.compress(lines(minutes(START, START + 4200))))
            (d / 'v.jsonl').write_bytes(lines(minutes(START, START + 4200, mem=lambda t: 55.5)))
            shown = io.StringIO()
            with contextlib.redirect_stdout(shown):
                code = tr.main(['--evidence', str(d / 'evidence.json'), '--history', f'sentry-1={d / "s.jsonl.gz"}',
                                '--history', f'validator-1={d / "v.jsonl"}', '--out', str(d / 'out.json')])
            self.assertIn('validator-1    hold:  30 samples, memory 55.5% median, 55.5% peak', shown.getvalue())
            self.assertEqual(code, 0)
            out = json.loads((d / 'out.json').read_text())
            self.assertEqual(out['schema'], tr.SCHEMA)
            self.assertEqual(sorted(out['hosts']), ['sentry-1', 'validator-1'])
            self.assertEqual(out['hosts']['validator-1']['hold']['memory_percent'], {'peak': 55.5, 'median': 55.5})

    def test_no_history_named_is_an_error(self):
        with tempfile.TemporaryDirectory() as d:
            (Path(d) / 'evidence.json').write_text(json.dumps(evidence()))
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(tr.main(['--evidence', str(Path(d) / 'evidence.json')]), 1)


if __name__ == '__main__':
    unittest.main()
