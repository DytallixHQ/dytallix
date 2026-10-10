"""The monitor job (monitoring v1, M4): what it reads, which approved rules
fire and resolve, what it sends and keeps. systemctl, curl, the clock and the
disk are stand-ins; the metric files, /proc and the state are real files."""
import datetime
import gzip
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'host'))
import monitor  # noqa: E402

THRESHOLDS = {
    'halt_seconds': 60, 'missed_blocks': 5, 'missed_blocks_window_seconds': 600, 'disk_free_warning_percent': 15,
    'disk_free_critical_percent': 5, 'rpc_failing_seconds': 180, 'staking_change_percent': 5,
    'staking_window_seconds': 3600, 'signature_failures': 10, 'signature_failures_window_seconds': 600,
    'restarts': 3, 'restarts_window_seconds': 900, 'metrics_max_age_seconds': 60, 'backup_max_age_seconds': 93600,
    'escalation_seconds': 900, 'history_days': 396,
}
SETTINGS = {'schema': 'dytallix.monitor-settings.v1', 'label': 'validator-1',
            'heartbeat_url': 'https://beat.example/ping/abc-123', 'alert_url': 'https://alerts.example/hook?token=x',
            'escalation_url': 'https://backup.example/hook'}
CEILING = 1_000_000_000
START = 1_800_000_000.0
GENESIS = 50_000_000_000_000


class MonitorJob(unittest.TestCase):
    role = 'validator'
    label = 'validator-1'

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.metrics, self.proc, self.state = root / 'metrics', root / 'proc', root / 'state'
        for directory in (self.metrics, self.proc / 'net', self.state):
            directory.mkdir(parents=True)
        self.settings = root / 'webhooks.json'
        self.backup = root / 'last-uploaded'
        self.config = root / 'monitor.json'
        self.config.write_text(json.dumps({
            'schema': 'dytallix.host-monitor.v1', 'label': self.label, 'role': self.role,
            'chain_id': 'dytallix-staging-1', 'metrics': str(self.metrics), 'data_disk': '/var/lib/dytallix',
            'node_unit': 'dytallix-node.service', 'settings': str(self.settings), 'state': str(self.state),
            'backup_state': str(self.backup) if self.role == 'sentry' else None,
            'status_url': 'http://10.20.0.12:8080/status' if self.role == 'endpoint' else None,
            'curl': '/usr/bin/curl', 'systemctl': '/usr/bin/systemctl', 'proc': str(self.proc),
            'thresholds': THRESHOLDS, 'emission_ceiling_udrt_per_block': CEILING}))
        self.now = START
        self.height = 100
        self.chain = {'emitted': 0, 'burned': 0, 'issued': 10**15, 'staked': 4 * 10**14, 'tx': 0, 'gov': 0,
                      'invalid': 0, 'missed': 0}
        self.invocation = 'a' * 32
        self.free = 50.0
        self.curl_exit = 0
        self.status = 200
        self.sent = []
        self.cpu_ticks = 0
        self.write_proc()

    # Stand-ins

    def write_proc(self):
        self.cpu_ticks += 6000
        (self.proc / 'stat').write_text(f'cpu  {self.cpu_ticks} 0 {self.cpu_ticks} {4 * self.cpu_ticks} 0 0 0 0 0 0\n')
        (self.proc / 'meminfo').write_text('MemTotal:       8000000 kB\nMemFree:  1 kB\nMemAvailable:   6000000 kB\n')
        n = self.cpu_ticks
        (self.proc / 'net' / 'dev').write_text(
            'Inter-|   Receive\n face |bytes\n'
            f'    lo: 999 0 0 0 0 0 0 0 999 0 0 0 0 0 0 0\n'
            f'  eth0: {n * 10} 0 0 0 0 0 0 0 {n * 20} 0 0 0 0 0 0 0\n')

    def write_metrics(self, app_age=5, engine_age=5, app=True, engine=True):
        c = self.chain
        total = GENESIS + c['emitted'] - c['burned'] + c.get('skew', 0)
        if app:
            (self.metrics / 'dytallix-app.prom').write_text('\n'.join([
                '# TYPE dytallix_app_height gauge', f'dytallix_app_height {self.height}',
                '# TYPE dytallix_app_transactions_total counter',
                f'dytallix_app_transactions_total{{kind="ordinary",result="ok"}} {c["tx"]}',
                f'dytallix_app_transactions_total{{kind="governance",result="ok"}} {c["gov"]}',
                f'dytallix_app_signature_verifications_total{{result="valid",scheme="ml_dsa_65"}} 900',
                f'dytallix_app_signature_verifications_total{{result="invalid",scheme="ml_dsa_65"}} {c["invalid"]}',
                f'dytallix_app_supply_udrt{{bucket="genesis"}} {GENESIS}',
                f'dytallix_app_supply_udrt{{bucket="emitted"}} {c["emitted"]}',
                f'dytallix_app_supply_udrt{{bucket="burned"}} {c["burned"]}',
                f'dytallix_app_supply_udrt{{bucket="total"}} {total}',
                f'dytallix_app_supply_udgt{{bucket="issued"}} {c["issued"]}',
                f'dytallix_app_supply_udgt{{bucket="staked"}} {c["staked"]}',
                f'dytallix_metrics_written_timestamp_seconds{{process="app"}} {self.now - app_age:.3f}', '']))
        else:
            (self.metrics / 'dytallix-app.prom').unlink(missing_ok=True)
        if engine:
            (self.metrics / 'dytallix-engine.prom').write_text('\n'.join([
                f'dytallix_engine_consensus_validator_missed_blocks{{validator_address="AB"}} {c["missed"]}',
                'dytallix_engine_p2p_peers 2', 'dytallix_engine_mempool_size 3',
                f'dytallix_metrics_written_timestamp_seconds{{process="engine"}} {self.now - engine_age:.11e}', '']))

    def run_command(self, args, input=None, capture_output=False, text=False):
        name = Path(args[0]).name
        if name == 'systemctl':
            return SimpleNamespace(returncode=0, stdout=f'ActiveState=active\nInvocationID={self.invocation}\n',
                                   stderr='')
        assert name == 'curl' and args[1:] == ['--config', '-'], args
        config = input if isinstance(input, str) else input.decode()
        url = next(line.split('"')[1] for line in config.splitlines() if line.startswith('url'))
        if 'write-out' in config:
            return SimpleNamespace(returncode=0, stdout=str(self.status), stderr='')
        data = next(line.split('"')[1] for line in config.splitlines() if line.startswith('data-binary'))
        payload = json.loads(Path(data[1:]).read_text())
        self.sent.append({'url': url, 'payload': payload, 'ok': self.curl_exit == 0})
        return SimpleNamespace(returncode=self.curl_exit, stdout=b'', stderr=b'')

    def statvfs(self, path):
        return SimpleNamespace(f_blocks=1000, f_bfree=int(self.free * 10), f_bavail=int(self.free * 10), f_frsize=4096)

    def tick(self, seconds=60, blocks=12, settings=True, **metrics):
        self.now += seconds
        self.height += blocks
        self.write_proc()
        self.write_metrics(**metrics)
        if settings and not self.settings.exists():
            self.settings.write_text(json.dumps(dict(SETTINGS, label=self.label)))
        before = len(self.sent)
        summary = monitor.run_once(self.config, run=self.run_command, clock=lambda: self.now, statvfs=self.statvfs,
                                   out=lambda _: None)
        return summary, self.sent[before:]

    def alerts(self, sent, kind='change'):
        return [(s['payload']['condition'], s['payload']['status']) for s in sent
                if s['payload']['schema'] == 'dytallix.alert.v1' and s['payload']['kind'] == kind]


class ValidatorTests(MonitorJob):
    def test_a_healthy_chain_sends_only_heartbeats_and_keeps_history(self):
        for _ in range(5):
            summary, sent = self.tick()
            self.assertEqual(summary['firing'], [])
            self.assertEqual(summary['heartbeat'], 'SENT')
            (beat,) = sent
            self.assertEqual(beat['url'], SETTINGS['heartbeat_url'])
            self.assertEqual(beat['payload']['host'], 'validator-1')
            self.assertEqual(beat['payload']['height'], self.height)
        state = json.loads((self.state / 'state.json').read_bytes())
        self.assertEqual(state['starts'], [])
        day = datetime.datetime.fromtimestamp(self.now, datetime.timezone.utc).date()
        lines = (self.state / 'history' / f'{day}.jsonl').read_text().splitlines()
        self.assertEqual(len(lines), 5)
        last = json.loads(lines[-1])
        self.assertEqual((last['h'], last['peers'], last['disk_free']), (self.height, 2, 50.0))
        self.assertEqual(last['cpu'], 33.3)
        self.assertEqual(last['mem'], 25.0)
        self.assertEqual((last['rx'], last['tx']), (1000, 2000))
        self.assertEqual(list((self.state / 'scratch').iterdir()), [], 'a payload file was left behind')

    def test_a_halt_fires_once_escalates_once_and_resolves(self):
        self.tick()
        summary, sent = self.tick(blocks=0)
        self.assertEqual(self.alerts(sent), [('consensus_halt', 'firing')])
        alert = next(s for s in sent if s['payload'].get('condition') == 'consensus_halt')
        self.assertEqual(alert['url'], SETTINGS['alert_url'])
        self.assertEqual((alert['payload']['severity'], alert['payload']['threshold']), (1, 60))
        self.assertTrue(alert['payload']['runbook'].endswith('monitoring.md#consensus-halt'))
        escalations = []
        for _ in range(16):
            summary, sent = self.tick(blocks=0)
            self.assertEqual(self.alerts(sent), [], 'a firing alert is sent only on its change')
            escalations += [s for s in sent if s['payload'].get('kind') == 'escalation']
        self.assertEqual([(e['url'], e['payload']['condition']) for e in escalations],
                         [(SETTINGS['escalation_url'], 'consensus_halt')])
        self.assertIn('consensus_halt', summary['firing'])
        _, sent = self.tick()
        self.assertEqual(self.alerts(sent), [('consensus_halt', 'resolved')])

    def test_failed_deliveries_wait_in_order_and_are_counted(self):
        self.tick()
        self.curl_exit = 7
        summary, _ = self.tick(blocks=0)
        self.assertEqual((summary['heartbeat'], summary['undelivered']), ('FAILED', 1))
        self.free = 4.0
        summary, _ = self.tick(blocks=0)
        self.assertEqual(summary['undelivered'], 3)
        self.curl_exit = 0
        summary, sent = self.tick(blocks=0)
        self.assertEqual(self.alerts(sent), [('consensus_halt', 'firing'), ('disk_critical', 'firing'),
                                             ('disk_low', 'firing')])
        beat = sent[-1]['payload']
        self.assertEqual((beat['undelivered'], beat['delivery_failures']), (0, 2))
        _, sent = self.tick(blocks=0)
        self.assertEqual(sent[-1]['payload']['delivery_failures'], 0)

    def test_without_settings_it_keeps_alerts_for_later(self):
        self.tick(settings=False)
        summary, sent = self.tick(blocks=0, settings=False)
        self.assertEqual((summary['heartbeat'], summary['undelivered'], sent), ('NO_SETTINGS', 1, []))
        _, sent = self.tick(blocks=0)
        self.assertEqual(self.alerts(sent), [('consensus_halt', 'firing')])

    def test_disk_levels(self):
        self.free = 14.9
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['disk_low'])
        self.free = 4.9
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['disk_critical', 'disk_low'])
        self.free = 30
        _, sent = self.tick()
        self.assertEqual(sorted(self.alerts(sent)), [('disk_critical', 'resolved'), ('disk_low', 'resolved')])

    def test_supply_rules(self):
        self.tick()
        # Emission within the per-block ceiling, burns with fees: quiet.
        self.chain.update(emitted=12 * CEILING, burned=500, tx=3)
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], [])
        # More than the ceiling for the blocks committed.
        self.chain['emitted'] += 12 * CEILING + 1
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['unexpected_mint_burn'])
        # A burn without a committed transaction.
        self.chain['burned'] += 1
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['unexpected_mint_burn'])
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], [])
        # The DRT total off its identity, then DGT issued changing, latch.
        self.chain['skew'] = 1
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['supply_invariant'])
        self.chain['skew'] = 0
        self.chain['issued'] += 1
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['supply_invariant'])
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['supply_invariant'])

    def test_a_restart_resets_counters_without_a_false_burn(self):
        self.chain.update(tx=50)
        self.tick()
        self.invocation = 'b' * 32
        self.chain.update(tx=0, burned=10)  # counters restart at zero
        summary, _ = self.tick()
        self.assertNotIn('unexpected_mint_burn', summary['firing'])

    def test_restart_loop(self):
        self.tick()
        for n, letter in enumerate('bcd'):
            self.invocation = letter * 32
            summary, _ = self.tick(seconds=240)
            self.assertEqual('restart_loop' in summary['firing'], n == 2)
        summary, _ = self.tick(seconds=900)
        self.assertNotIn('restart_loop', summary['firing'])

    def test_stale_or_missing_metrics(self):
        self.tick()
        summary, _ = self.tick(engine_age=61)
        self.assertIn('stale_metrics', summary['firing'])
        summary, _ = self.tick()
        self.assertNotIn('stale_metrics', summary['firing'])
        summary, _ = self.tick(app=False)
        self.assertIn('stale_metrics', summary['firing'])

    def test_missed_blocks_and_signature_failures_in_their_windows(self):
        self.tick()
        self.chain.update(missed=4, invalid=9)
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], [])
        self.chain.update(missed=5, invalid=10)
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], ['missed_blocks', 'signature_failures'])
        for _ in range(10):
            summary, _ = self.tick()
        self.assertEqual(summary['firing'], [], 'outside the ten-minute windows')

    def test_staking_change_over_an_hour_and_governance_events(self):
        for _ in range(60):
            self.tick()
        self.chain['staked'] = int(self.chain['staked'] * 1.06)
        summary, _ = self.tick()
        self.assertIn('staking_change', summary['firing'])
        self.chain['gov'] += 1
        summary, sent = self.tick()
        self.assertIn('governance_event', summary['firing'])
        self.assertEqual([s['payload']['severity'] for s in sent if s['payload'].get('condition') == 'governance_event'],
                         [3])
        summary, _ = self.tick()
        self.assertNotIn('governance_event', summary['firing'])

    def test_history_rolls_over_compresses_and_expires(self):
        history = self.state / 'history'
        history.mkdir()
        today = datetime.datetime.fromtimestamp(START + 60, datetime.timezone.utc).date()
        old = today - datetime.timedelta(days=397)
        kept = today - datetime.timedelta(days=396)
        yesterday = today - datetime.timedelta(days=1)
        for day in (old, kept):
            (history / f'{day}.jsonl.gz').write_bytes(gzip.compress(b'{}\n'))
        (history / f'{yesterday}.jsonl').write_text('{"t":1}\n')
        self.tick()
        names = sorted(p.name for p in history.iterdir())
        self.assertEqual(names, sorted([f'{kept}.jsonl.gz', f'{yesterday}.jsonl.gz', f'{today}.jsonl']))
        self.assertEqual(gzip.decompress((history / f'{yesterday}.jsonl.gz').read_bytes()), b'{"t":1}\n')

    def test_refusals(self):
        self.tick()
        for change, message in ((dict(label='sentry-1'), 'for sentry-1'),
                                (dict(alert_url='http://alerts.example/x'), 'https'),
                                (dict(heartbeat_url='https://beat.example/"x'), 'unexpected characters'),
                                (dict(escalation_url='https://user:pw@backup.example/'), 'plain URL')):
            with self.subTest(change=change):
                self.settings.write_text(json.dumps(dict(SETTINGS, **change)))
                summary, sent = self.tick()
                self.assertEqual(summary['heartbeat'], 'NO_SETTINGS')
                self.assertTrue(any(message in e for e in summary['errors']), summary['errors'])
                self.assertEqual(sent, [])
        # A stand-in receiver on this host may use plain HTTP.
        self.settings.write_text(json.dumps(dict(SETTINGS, alert_url='http://127.0.0.1:9100/alert')))
        summary, _ = self.tick()
        self.assertEqual(summary['heartbeat'], 'SENT')
        config = json.loads(self.config.read_bytes())
        for name, bad in (('halt_seconds', 0), ('disk_free_critical_percent', 20), ('history_days', '396')):
            with self.subTest(threshold=name):
                self.config.write_text(json.dumps(dict(config, thresholds=dict(THRESHOLDS, **{name: bad}))))
                with self.assertRaises(monitor.Failed):
                    monitor.load(self.config)


class RunbookTests(unittest.TestCase):
    def test_every_alert_links_to_a_runbook_section(self):
        runbook = (HERE.parents[1] / 'docs' / 'operations' / 'monitoring.md').read_text()
        anchors = {line[4:].strip().lower().replace(' ', '-') for line in runbook.splitlines() if line.startswith('### ')}
        self.assertTrue(monitor.RUNBOOK.endswith('node/docs/operations/monitoring.md'))
        for name in list(monitor.RULES) + ['validator_outage', 'monitoring_down']:
            self.assertIn(name.replace('_', '-'), anchors)


class SentryTests(MonitorJob):
    role, label = 'sentry', 'sentry-1'

    def test_stale_backup_and_role_scope(self):
        summary, _ = self.tick()
        self.assertEqual(summary['firing'], [])
        summary, _ = self.tick(seconds=93600)
        self.assertEqual(summary['firing'], ['stale_backup'])
        self.backup.write_text('17280\n')
        os.utime(self.backup, (self.now, self.now))
        summary, sent = self.tick()
        self.assertEqual(self.alerts(sent), [('stale_backup', 'resolved')])
        state = json.loads((self.state / 'state.json').read_bytes())
        self.assertNotIn('missed_blocks', state['alerts'])
        self.assertIn('supply_invariant', state['alerts'])


class EndpointTests(MonitorJob):
    role, label = 'endpoint', 'endpoint-1'

    def test_rpc_failing_for_three_minutes(self):
        self.tick()
        self.status = 503
        for n in range(4):
            summary, _ = self.tick()
            self.assertEqual('rpc_failing' in summary['firing'], n == 3)
        day = datetime.datetime.fromtimestamp(self.now, datetime.timezone.utc).date()
        last = json.loads((self.state / 'history' / f'{day}.jsonl').read_text().splitlines()[-1])
        self.assertEqual(last['rpc'], 503)
        self.status = 200
        summary, _ = self.tick()
        self.assertNotIn('rpc_failing', summary['firing'])
        state = json.loads((self.state / 'state.json').read_bytes())
        self.assertNotIn('supply_invariant', state['alerts'])


if __name__ == '__main__':
    unittest.main()
