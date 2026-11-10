#!/usr/bin/env python3
"""Tests for fault_alerts.py and the stand-in's relay (alert_standin.py)."""
from http.server import ThreadingHTTPServer
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest import mock
import urllib.request

import alert_standin
import fault_alerts
import faults
import test_results

NATIVE = test_results.NATIVE
T0 = faults.instant('2026-11-10T10:00:00+00:00').timestamp()


def event(received, to, host, condition=None, status=None):
    body = {'host': host}
    if to != 'heartbeat':
        body.update(kind='change', condition=condition, status=status, severity=1)
    return {'received': received, 'to': to, 'body': body}


def events_raw(*events):
    return b''.join(json.dumps(e).encode() + b'\n' for e in events)


def window(start, end, cleared=None):
    return {'evidence_sha256': 'ee' * 32, 'injected_at': start, 'cleared_at': cleared, 'recovered_at': end}


def all_windows():
    return {'partition': window(T0, T0 + 110, T0 + 90), 'host_loss': window(T0 + 1000, T0 + 1065, T0 + 1030),
            'sentry_rebuild': window(T0 + 2000, T0 + 2400), 'validator_host': window(T0 + 3000, T0 + 3300)}


# A run in which every in-scope fault raised an alert that resolved.
EVENTS = events_raw(
    event(T0 - 10, 'heartbeat', 'sentry-1'),
    event(T0 + 70, 'alert', 'sentry-1', 'consensus_halt', 'firing'),
    event(T0 + 100, 'alert', 'sentry-1', 'consensus_halt', 'resolved'),
    event(T0 + 1040, 'alert', 'sentry-1', 'consensus_halt', 'firing'),
    event(T0 + 1080, 'alert', 'sentry-1', 'consensus_halt', 'resolved'),
    event(T0 + 2190, 'service', 'sentry-1', 'monitoring_down', 'firing'),
    event(T0 + 2420, 'service', 'sentry-1', 'monitoring_down', 'resolved'),
    event(T0 + 3070, 'alert', 'sentry-1', 'consensus_halt', 'firing'),
    event(T0 + 3190, 'service', 'validator-1', 'monitoring_down', 'firing'),
    event(T0 + 3290, 'alert', 'sentry-1', 'consensus_halt', 'resolved'),
    event(T0 + 3320, 'service', 'validator-1', 'monitoring_down', 'resolved'))
DECIDED = {'partition': [['sentry-1', 'consensus_halt']], 'host_loss': [['sentry-1', 'consensus_halt']],
           'sentry_rebuild': [['sentry-1', 'monitoring_down']],
           'validator_host': [['sentry-1', 'consensus_halt'], ['validator-1', 'monitoring_down']]}


def build(raw=EVENTS, windows=None):
    return fault_alerts.build('dytallix-staging-2', NATIVE, windows or all_windows(), raw, '2026-11-10T12:00:00Z')


class Episodes(unittest.TestCase):
    def test_each_firing_pairs_with_the_next_resolve_of_its_host_and_condition(self):
        found = fault_alerts.episodes(fault_alerts.parse_events(EVENTS + b'{"half a line'))
        self.assertEqual(len(found), 5)
        self.assertEqual(found[0], {'host': 'sentry-1', 'condition': 'consensus_halt', 'severity': 1,
                                    'fired_at': T0 + 70, 'resolved_at': T0 + 100})
        self.assertEqual([(e['host'], e['condition'], e['resolved_at']) for e in found[3:5]],
                         [('sentry-1', 'consensus_halt', T0 + 3290), ('validator-1', 'monitoring_down', T0 + 3320)])

    def test_a_repeated_firing_or_a_stray_resolve_adds_nothing_and_an_open_one_is_firing(self):
        raw = events_raw(event(1, 'alert', 'a', 'x', 'resolved'), event(2, 'alert', 'a', 'x', 'firing'),
                         event(3, 'alert', 'a', 'x', 'firing'), event(4, 'escalation', 'a', 'x', 'firing'))
        found = fault_alerts.episodes(fault_alerts.parse_events(raw))
        self.assertEqual([(e['fired_at'], e['resolved_at']) for e in found], [(2, None)])
        self.assertEqual(fault_alerts.firing(fault_alerts.parse_events(raw)), [('a', 'x')])


class Windows(unittest.TestCase):
    def test_each_window_comes_from_its_drills_evidence(self):
        partition = json.loads(test_results.record([test_results.row('latency', 9), test_results.row('partition', 21)]))
        host_loss_row = dict(test_results.row('partition', 35), name='host_loss')
        lost = json.loads(test_results.record([host_loss_row], target=600))
        restore = json.loads(test_results.rebuild(400))
        validator = json.loads(test_results.host_loss(300))
        found = fault_alerts.windows(partition, lost, restore, validator)
        self.assertEqual(list(found), list(fault_alerts.FAULTS))
        self.assertEqual(found['partition']['recovered_at'], faults.instant('2026-11-03T10:00:00+00:00').timestamp())
        self.assertEqual(found['partition']['evidence_sha256'], partition['sha256'])
        self.assertAlmostEqual(found['partition']['recovered_at'] - found['partition']['cleared_at'], 21)
        self.assertAlmostEqual(found['sentry_rebuild']['recovered_at'] - found['sentry_rebuild']['injected_at'], 400)
        self.assertAlmostEqual(found['validator_host']['recovered_at'] - found['validator_host']['injected_at'], 300)
        with self.assertRaises(fault_alerts.Failed):
            fault_alerts.windows(partition, partition, restore, validator)  # no host_loss scenario


class Judge(unittest.TestCase):
    def test_while_the_alerts_are_not_decided_every_fault_is_inconclusive_with_what_it_saw(self):
        self.assertTrue(all(v is None for v in fault_alerts.EXPECTED.values()))
        record = build()
        self.assertEqual(record['status'], fault_alerts.INCONCLUSIVE)
        self.assertEqual({r['status'] for r in record['faults']}, {fault_alerts.INCONCLUSIVE})
        rows = {r['fault']: r for r in record['faults']}
        self.assertEqual([(e['host'], e['condition']) for e in rows['validator_host']['observed']],
                         [('sentry-1', 'consensus_halt'), ('validator-1', 'monitoring_down')])
        self.assertIn('not decided; seen: sentry-1 monitoring_down resolved', rows['sentry_rebuild']['detail'])
        self.assertEqual(fault_alerts.verify(json.loads(json.dumps(record)), NATIVE), fault_alerts.INCONCLUSIVE)

    def test_once_decided_a_fault_passes_only_when_its_alerts_fired_in_its_window_and_resolved(self):
        with mock.patch.dict(fault_alerts.EXPECTED, DECIDED):
            record = build()
            self.assertEqual(record['status'], fault_alerts.PASS)
            self.assertEqual(fault_alerts.verify(record, NATIVE), fault_alerts.PASS)
            # The validator's monitoring_down never resolved; host loss's alert came after its recovery.
            raw = EVENTS.replace(b'"monitoring_down", "status": "resolved"', b'"monitoring_down", "status": "other"')
            windows = dict(all_windows(), host_loss=window(T0 + 1000, T0 + 1030))
            rows = {r['fault']: r for r in build(raw, windows)['faults']}
            self.assertEqual(rows['validator_host']['status'], fault_alerts.FAIL)
            self.assertIn('not fired and resolved: validator-1 monitoring_down', rows['validator_host']['detail'])
            self.assertEqual(rows['host_loss']['status'], fault_alerts.FAIL)
            self.assertEqual(rows['partition']['status'], fault_alerts.PASS)

    def test_a_fault_with_no_recovery_is_inconclusive(self):
        with mock.patch.dict(fault_alerts.EXPECTED, DECIDED):
            rows = {r['fault']: r for r in build(windows=dict(all_windows(), partition=window(T0, None)))['faults']}
        self.assertEqual(rows['partition']['status'], fault_alerts.INCONCLUSIVE)
        self.assertEqual(rows['partition']['observed'], [])


class Verify(unittest.TestCase):
    def test_an_edit_another_genesis_or_undecided_alerts_are_refused(self):
        record = build()
        edited = json.loads(json.dumps(record))
        edited['faults'][0]['observed'] = []
        with self.assertRaises(fault_alerts.Failed):
            fault_alerts.verify(edited, NATIVE)
        with self.assertRaises(fault_alerts.Failed):
            fault_alerts.verify(record, b'{}')
        # A record judged against alerts nobody decided, digest made to match.
        with mock.patch.dict(fault_alerts.EXPECTED, DECIDED):
            decided = build()
        with self.assertRaisesRegex(fault_alerts.Failed, 'other alerts than the decided'):
            fault_alerts.verify(decided, NATIVE)

    def test_an_observed_alert_outside_its_window_is_refused(self):
        record = build()
        record['faults'][0]['observed'].append({'host': 'sentry-1', 'condition': 'disk_low', 'severity': 2,
                                                'fired_at': T0 + 5000, 'resolved_at': None})
        record['faults'][0]['detail'] = fault_alerts.evaluate('partition', record['faults'][0],
                                                              record['faults'][0]['observed'], None)[1]
        body = {k: v for k, v in record.items() if k != 'sha256'}
        record['sha256'] = fault_alerts.digest(fault_alerts.canonical(body))
        with self.assertRaisesRegex(fault_alerts.Failed, 'outside its window'):
            fault_alerts.verify(record, NATIVE)


class Relay(unittest.TestCase):
    def serve(self, handler):
        server = ThreadingHTTPServer(('127.0.0.1', 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return server.server_address[1]

    def test_the_relay_forwards_path_and_body_to_the_runners_service(self):
        with tempfile.TemporaryDirectory() as root:
            service = alert_standin.Service(root, 180)
            port = self.serve(alert_standin.handler(service))
            relay = self.serve(alert_standin.handler(None, alert_standin.Relay(f'http://127.0.0.1:{port}')))
            body = json.dumps({'host': 'sentry-1', 'time': 1, 'firing': []}).encode()
            request = urllib.request.Request(f'http://127.0.0.1:{relay}/heartbeat', data=body, method='POST')
            with urllib.request.urlopen(request, timeout=5) as response:
                self.assertEqual(response.status, 200)
            [line] = (Path(root) / 'events.jsonl').read_text().splitlines()
            self.assertEqual(json.loads(line)['to'], 'heartbeat')
            self.assertEqual(json.loads(line)['body']['host'], 'sentry-1')

    def test_an_unreachable_service_answers_502(self):
        self.assertEqual(alert_standin.Relay('http://127.0.0.1:9', timeout=1).forward('/alert', b'{}'), 502)


if __name__ == '__main__':
    unittest.main()
