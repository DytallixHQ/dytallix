"""results.py: staging.t05.capacity from the T05 stepped load run, and one staging summary a day."""
import contextlib
from datetime import datetime, timezone
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import load
import results
import test_load

START = datetime(2026, 11, 18, 15, 0, tzinfo=timezone.utc).timestamp()
GENESIS = json.dumps({'chain_id': test_load.CHAIN, 'accounts': []}).encode()


def stamp(seconds, nanos=''):
    return datetime.fromtimestamp(seconds, timezone.utc).strftime('%Y-%m-%dT%H:%M:%S') + nanos + 'Z'


def t05_run(sent=10, refused_in_hold=0, chain=test_load.CHAIN):
    """A drive --steps record as load_workers.py writes it: one 60 s step at 2 a second
    (10 a block), `sent` launched a block, each committed in the next block."""
    rows, times = [], {}
    for k in range(12):
        height = 101 + k
        times[str(height)] = stamp(START + 5 * k + 5, '.123456789')
        for j in range(sent):
            refused = k == 6 and j < refused_in_hold
            row = test_load.tx_row('run', 'send_drt', status='refused' if refused else 'committed',
                                   block=height, submit=height - 1)
            row['submitted_at'] = START + 5 * k + 0.1
            rows.append(row)
    raw = load.rows_jsonl(rows)
    ledger = load.ledger(rows, 100, 10)
    charged = sum(int(e['charge_udrt']) for e in ledger['entries'])
    settings = dict(test_load.SETTINGS, max_refused=None, steps=[[2.0, 60.0]])
    window = {'workers': [{'window': {'started': datetime.fromtimestamp(START, timezone.utc)
                                      .strftime('%Y-%m-%dT%H:%M:%S.%fZ')}}]}
    record = load.evidence(chain, settings, {}, raw, ledger, test_load.supply(0, 100),
                           test_load.supply(charged, 113), times, {}, window, stamp(START), stamp(START + 70))
    return json.dumps(record).encode(), raw


@mock.patch.object(results, 'CAPACITY_PER_BLOCK', 10)
@mock.patch.object(results, 'CAPACITY_HOLD_SECONDS', 60.0)
class Capacity(unittest.TestCase):
    def kpi(self, run, genesis=GENESIS):
        return results.summarize_staging(genesis, capacity=run)['kpis'][0]

    def test_a_held_rate_with_nothing_lost_passes(self):
        run = t05_run()
        summary = results.summarize_staging(GENESIS, capacity=run)
        self.assertEqual(summary['kpis'], [{'id': 'staging.t05.capacity', 'status': 'pass', 'value': 10.0,
                                            'unit': 'count', 'target': None}])
        self.assertEqual((summary['scope'], summary['date']), ('staging', '2026-11-18'))
        self.assertEqual(summary['sources'], [{'tool': 'load', 'sha256': results.digest(run[0]),
                                               'chain_id': test_load.CHAIN,
                                               'native_genesis_sha256': None,
                                               'status': load.verify(json.loads(run[0]), run[1])}])
        self.assertEqual(results.summary_problems(summary), [])

    def test_a_source_limited_run_stays_in_progress_at_its_highest_rate(self):
        kpi = self.kpi(t05_run(sent=5))
        self.assertEqual((kpi['status'], kpi['value']), ('in_progress', 5.0))

    def test_a_refusal_in_the_hold_fails(self):
        kpi = self.kpi(t05_run(refused_in_hold=1))
        self.assertEqual(kpi['status'], 'fail')

    def test_an_edited_run_or_another_chain_is_refused(self):
        raw, rows = t05_run()
        record = json.loads(raw)
        record['block_times']['101'] = stamp(START)
        with self.assertRaisesRegex(results.Failed, 'does not verify'):
            self.kpi((json.dumps(record).encode(), rows))
        other = json.dumps({'chain_id': 'dytallix-staging-3'}).encode()
        with self.assertRaisesRegex(results.Failed, 'not the genesis chain'):
            self.kpi(t05_run(), other)

    def test_a_run_without_steps_is_refused(self):
        raw, rows = t05_run()
        record = json.loads(raw)
        del record['settings']['steps']
        record = {k: v for k, v in record.items() if k != 'sha256'}
        record['sha256'] = load.sha256(load.canonical(record))
        with self.assertRaisesRegex(results.Failed, 'cannot be judged'):
            self.kpi((json.dumps(record).encode(), rows))

    def test_the_command_writes_the_staging_summary(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            raw, rows = t05_run()
            (d / 'run').mkdir()
            (d / 'run' / 'evidence.json').write_bytes(raw)
            (d / 'run' / 'transactions.jsonl').write_bytes(rows)
            (d / 'genesis.json').write_bytes(GENESIS)
            argv = ['results.py', 'summarize', 'staging', '--t05', str(d / 'run' / 'evidence.json'),
                    '--genesis', str(d / 'genesis.json'), '--root', str(d / 'root')]
            shown = io.StringIO()
            with mock.patch.object(sys, 'argv', argv), contextlib.redirect_stdout(shown):
                self.assertEqual(results.main(), 0)
            self.assertIn('staging.t05.capacity pass', shown.getvalue())
            written = json.loads((d / 'root' / 'launch/simulations/results/staging/2026-11-18.json').read_bytes())
            self.assertEqual(written['kpis'][0]['value'], 10.0)
            with mock.patch.object(sys, 'argv', argv[:3] + ['--hosts', 'h.json'] + argv[5:]), \
                    contextlib.redirect_stderr(io.StringIO()) as said:
                self.assertEqual(results.main(), 1)
            self.assertIn('--hosts needs --staging and --recovery', said.getvalue())


class OneSummaryADay(unittest.TestCase):
    def summary(self, *ids):
        return {'schema': results.SCHEMA, 'scope': 'staging', 'date': '2026-11-18',
                'kpis': [{'id': kid, 'status': 'in_progress', 'value': None, 'unit': 'count',
                          'target': 2 if kid == 'staging.hosts.ready' else None} for kid in ids], 'sources': []}

    def test_a_new_summary_of_the_day_may_not_drop_a_kpi(self):
        with tempfile.TemporaryDirectory() as root:
            path = results.write_summary(self.summary('staging.hosts.ready'), root)
            with self.assertRaisesRegex(results.Failed, 'already reports staging.hosts.ready'):
                results.write_summary(self.summary('staging.t05.capacity'), root)
            self.assertIn('staging.hosts.ready', path.read_text())
            both = self.summary('staging.hosts.ready', 'staging.t05.capacity')
            self.assertEqual(results.write_summary(both, root), path)
            self.assertEqual(json.loads(path.read_bytes()), both)


if __name__ == '__main__':
    unittest.main()
