"""The checkpoint recorder (simulation plan, T7): its check table, how a
checkpoint's status follows from its checks, the window and chaining rules,
its digest-bound evidence, and that only the R07 schedule verifies as the
dress rehearsal. The real reconciliation and load evidence are bound and
re-verified in the Host install workflow's checkpoint step; here the bound
tools' own `verify` is stubbed so the tests cover T7's logic alone."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import checkpoints as cp

ENGINE = json.dumps({'genesis_time': '2026-10-12T14:00:00Z', 'chain_id': 'dytallix-staging-1'}).encode()
NATIVE = json.dumps({'accounts': [{'address': 'a', 'balances': {'udrt': '1000', 'udgt': '600'}}]}).encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


class CheckTable(unittest.TestCase):
    def test_the_table_is_exactly_the_brief_s_checks(self):
        register = json.loads(cp.REGISTER.read_bytes())
        strings = {c for item in register['final_dress_rehearsal_checkpoints'] for c in item['checks_verbatim']}
        self.assertEqual(set(cp.CHECK_TABLE), strings)

    def test_every_r07_label_has_an_offset(self):
        for row in cp.schedule('R07'):
            self.assertIn(row['label'], cp.OFFSETS)

    def test_an_unregistered_check_is_refused(self):
        with self.assertRaises(cp.Failed):
            cp.category('Verify something nobody approved.')


class Evaluators(unittest.TestCase):
    """The raw-reading evaluators, outside any schedule."""

    def evaluate(self, check, row, window=('2026-10-12T14:00:00Z', None)):
        return cp.evaluate(check, row, Path('.'), window, sha256(NATIVE), NATIVE)

    def test_participation(self):
        self.assertEqual(self.evaluate('Verify expected validator participation.',
                                       {'inputs': {'expected': ['v1'], 'effective': ['v1']}})[0], cp.PASS)
        self.assertEqual(self.evaluate('Verify expected validator participation.',
                                       {'inputs': {'expected': ['v1', 'v2'], 'effective': ['v1']}})[0], cp.FAIL)
        self.assertEqual(self.evaluate('Verify expected validator participation.', {})[0], cp.INCONCLUSIVE)

    def test_rpc_and_wallet(self):
        self.assertEqual(self.evaluate('Verify RPC.', {'inputs': {'reachable': True, 'height': 9}})[0], cp.PASS)
        self.assertEqual(self.evaluate('Verify RPC.', {'inputs': {'reachable': False}})[0], cp.FAIL)
        self.assertEqual(self.evaluate('Verify wallets.', {'inputs': {'reachable': True}})[0], cp.PASS)

    def test_genesis_and_binary_digest(self):
        self.assertEqual(self.evaluate('Verify genesis.', {'inputs': {'observed': sha256(NATIVE)}})[0], cp.PASS)
        self.assertEqual(self.evaluate('Verify genesis.', {'inputs': {'observed': 'deadbeef'}})[0], cp.FAIL)
        same = {'dytallix': 'aa'}
        self.assertEqual(self.evaluate('Verify binaries.',
                                       {'inputs': {'observed': same, 'manifest': same}})[0], cp.PASS)
        self.assertEqual(self.evaluate('Verify binaries.',
                                       {'inputs': {'observed': {'dytallix': 'aa'}, 'manifest': {'dytallix': 'bb'}}})[0],
                         cp.FAIL)

    def test_unsourced_checks_name_their_tool(self):
        for check, tool in (('Start validators.', 'T2'), ('Verify backups.', 'T9'),
                            ('Review logs.', 'operator'), ('Verify all genesis allocations.', 'T1')):
            status, detail = self.evaluate(check, {})
            self.assertEqual(status, cp.INCONCLUSIVE)
            self.assertIn(tool, detail)


def write_bind(base, tool, finished, name='reconcile.json'):
    """Write a stand-in sub-evidence file and return the bind that names it."""
    record = {'finished': finished, 'summary': {'block_interval_s': {'n': 20, 'p50': 5.0, 'p99': 9.0}}}
    data = json.dumps(record).encode()
    (base / name).write_bytes(data)
    bind = {'tool': tool, 'evidence': name, 'evidence_sha256': sha256(data)}
    if tool == 'load':
        (base / 'rows.jsonl').write_bytes(b'')
        bind.update(rows='rows.jsonl', rows_sha256=sha256(b''))
    return bind


# A short, CI-shaped schedule of sourceable R07 checks, one checkpoint at T0.
SCHED = [{'label': 'T ZERO', 'offset_seconds': 0,
          'checks': ['Verify token supply.', 'Confirm block production.',
                     'Verify expected validator participation.', 'Verify RPC.']}]


class Recording(unittest.TestCase):
    def setUp(self):
        self.base = Path(tempfile.mkdtemp())

    def record(self, readings, actual='2026-10-12T14:00:05Z', schedule=SCHED):
        return cp.record_checkpoint(schedule, 0, readings, self.base, NATIVE, ENGINE, '', actual)

    def run_of(self, checkpoints, schedule=SCHED):
        return cp.build_run('R07', schedule, 'dytallix-staging-1', NATIVE, ENGINE, checkpoints)

    def all_pass_readings(self, reconcile_status=cp.PASS, finished='2026-10-12T14:00:03Z'):
        return {
            'Verify token supply.': {'bind': write_bind(self.base, 'reconcile', finished)},
            'Confirm block production.': {'bind': write_bind(self.base, 'load', finished, 'load.json')},
            'Verify expected validator participation.': {'inputs': {'expected': ['v1'], 'effective': ['v1']}},
            'Verify RPC.': {'inputs': {'reachable': True, 'height': 3}},
        }

    def test_a_clean_checkpoint_passes_and_verifies(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            checkpoint = self.record(self.all_pass_readings())
            self.assertEqual(checkpoint['status'], cp.PASS)
            run = self.run_of([checkpoint])
            self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE), cp.PASS)

    def test_a_reconciliation_fail_fails_the_run(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.FAIL), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            checkpoint = self.record(self.all_pass_readings())
            self.assertEqual(checkpoint['status'], cp.FAIL)
            run = self.run_of([checkpoint])
            self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE), cp.FAIL)

    def test_bound_evidence_outside_the_window_does_not_count(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            # finished a day later than a one-checkpoint window that ends at +inf still
            # counts, so give the schedule a second checkpoint to bound the window.
            schedule = [SCHED[0], {'label': 'T PLUS 1 HOUR', 'offset_seconds': 3600, 'checks': ['Verify RPC.']}]
            readings = self.all_pass_readings(finished='2026-10-12T16:00:00Z')
            checkpoint = cp.record_checkpoint(schedule, 0, readings, self.base, NATIVE, ENGINE, '',
                                              '2026-10-12T14:00:05Z')
            supply = next(r for r in checkpoint['checks'] if r['check'] == 'Verify token supply.')
            self.assertEqual(supply['status'], cp.INCONCLUSIVE)

    def test_a_tampered_inner_check_is_caught(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            run = self.run_of([self.record(self.all_pass_readings())])
            run['checkpoints'][0]['checks'][0]['status'] = cp.FAIL
            # Recompute the inner and top digests so only the reading is inconsistent.
            cpt = run['checkpoints'][0]
            body = {k: v for k, v in cpt.items() if k != 'sha256'}
            cpt['sha256'] = sha256(cp.canonical(body))
            body = {k: v for k, v in run.items() if k != 'sha256'}
            run['sha256'] = sha256(cp.canonical(body))
            with self.assertRaises(cp.Failed):
                cp.verify(run, self.base, NATIVE, ENGINE)

    def test_a_swapped_engine_genesis_is_caught(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            run = self.run_of([self.record(self.all_pass_readings())])
            other = json.dumps({'genesis_time': '2026-10-12T13:00:00Z'}).encode()
            with self.assertRaises(cp.Failed):
                cp.verify(run, self.base, NATIVE, other)

    def test_a_missing_sub_evidence_file_is_not_skipped(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            run = self.run_of([self.record(self.all_pass_readings())])
            (self.base / 'reconcile.json').unlink()
            with self.assertRaises((cp.Failed, OSError)):
                cp.verify(run, self.base, NATIVE, ENGINE)

    def test_an_incomplete_run_is_in_progress(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            schedule = [SCHED[0], {'label': 'T PLUS 1 HOUR', 'offset_seconds': 3600, 'checks': ['Verify RPC.']}]
            readings = self.all_pass_readings()
            checkpoint = cp.record_checkpoint(schedule, 0, readings, self.base, NATIVE, ENGINE, '',
                                              '2026-10-12T14:00:05Z')
            run = cp.build_run('R07', schedule, 'dytallix-staging-1', NATIVE, ENGINE, [checkpoint])
            self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE), cp.INCONCLUSIVE)

    def test_a_short_schedule_is_not_the_dress_rehearsal(self):
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            run = self.run_of([self.record(self.all_pass_readings())])
            self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE), cp.PASS)
            with self.assertRaises(cp.Failed):
                cp.verify(run, self.base, NATIVE, ENGINE, qualifying='R07')

    def test_a_non_integrity_fail_fails_the_run_without_stopping_it(self):
        # An RPC failure at the first of two checkpoints: the run continues
        # (no raise) and verifies to FAIL, not an error.
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            schedule = [{'label': 'T ZERO', 'offset_seconds': 0, 'checks': ['Verify RPC.']},
                        {'label': 'T PLUS 1 HOUR', 'offset_seconds': 3600, 'checks': ['Verify RPC.']}]
            first = cp.record_checkpoint(schedule, 0, {'Verify RPC.': {'inputs': {'reachable': False}}},
                                         self.base, NATIVE, ENGINE, '', '2026-10-12T14:00:05Z')
            self.assertEqual(first['status'], cp.FAIL)
            second = cp.record_checkpoint(schedule, 1, {'Verify RPC.': {'inputs': {'reachable': True, 'height': 1}}},
                                          self.base, NATIVE, ENGINE, first['sha256'], '2026-10-12T15:00:05Z')
            run = cp.build_run('R07', schedule, 'dytallix-staging-1', NATIVE, ENGINE, [first, second])
            self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE), cp.FAIL)

    def test_an_integrity_fail_must_stop_the_run(self):
        # A supply failure at the first of two checkpoints is an integrity
        # failure: a record that keeps going is refused.
        with mock.patch.object(cp.reconcile, 'verify', return_value=cp.FAIL), \
             mock.patch.object(cp.load_tool, 'verify', return_value=cp.PASS):
            schedule = [{'label': 'T ZERO', 'offset_seconds': 0, 'checks': ['Verify token supply.']},
                        {'label': 'T PLUS 1 HOUR', 'offset_seconds': 3600, 'checks': ['Verify token supply.']}]
            bind = {'Verify token supply.': {'bind': write_bind(self.base, 'reconcile', '2026-10-12T14:00:03Z')}}
            first = cp.record_checkpoint(schedule, 0, bind, self.base, NATIVE, ENGINE, '', '2026-10-12T14:00:05Z')
            self.assertEqual(first['status'], cp.FAIL)
            bind2 = {'Verify token supply.': {'bind': write_bind(self.base, 'reconcile', '2026-10-12T15:00:03Z')}}
            second = cp.record_checkpoint(schedule, 1, bind2, self.base, NATIVE, ENGINE, first['sha256'],
                                          '2026-10-12T15:00:05Z')
            run = cp.build_run('R07', schedule, 'dytallix-staging-1', NATIVE, ENGINE, [first, second])
            with self.assertRaises(cp.Failed):
                cp.verify(run, self.base, NATIVE, ENGINE)

    def test_a_real_reconciliation_is_bound_and_re_verified_unstubbed(self):
        # No mocks: a genuine reconcile PASS record, bound and re-verified
        # through checkpoints with reconcile's own verify.
        import test_reconcile as tr
        rec = tr.rc.evidence('c', tr.observations(), tr.GENESIS_BYTES,
                             '2026-10-12T14:00:01Z', '2026-10-12T14:00:03Z')
        self.assertEqual(rec['status'], cp.PASS)
        (self.base / 'reconcile.json').write_bytes(json.dumps(rec).encode())
        bind = {'tool': 'reconcile', 'evidence': 'reconcile.json',
                'evidence_sha256': sha256((self.base / 'reconcile.json').read_bytes())}
        schedule = [{'label': 'T ZERO', 'offset_seconds': 0, 'checks': ['Verify token supply.']}]
        checkpoint = cp.record_checkpoint(schedule, 0, {'Verify token supply.': {'bind': bind}},
                                          self.base, tr.GENESIS_BYTES, ENGINE, '', '2026-10-12T14:00:03Z')
        run = cp.build_run('R07', schedule, 'c', tr.GENESIS_BYTES, ENGINE, [checkpoint])
        self.assertEqual(cp.verify(run, self.base, tr.GENESIS_BYTES, ENGINE), cp.PASS)

    def test_a_real_load_run_is_bound_and_re_verified_unstubbed(self):
        # No mocks: a genuine load PASS record (test_load's fixtures) bound for
        # the liveness and transaction checks and re-verified with load's verify.
        import load as ld
        import test_load as tl
        rows = [tl.tx_row('setup', 'fund', created=8, block=11), tl.tx_row('run', 'send_drt', block=11),
                tl.tx_row('run', 'create', block=12, created=1), tl.tx_row('run', 'claim', block=13, submit=11)]
        raw = ld.rows_jsonl(rows)
        ledger = ld.ledger(rows, 10, 10)
        rec = ld.evidence(tl.CHAIN, tl.SETTINGS, {}, raw, ledger, tl.supply(0, 10),
                          tl.supply(4000000 + 90, 16), tl.TIMES, {}, {},
                          '2026-10-12T14:00:01Z', '2026-10-12T14:00:03Z')
        self.assertEqual(rec['status'], ld.PASS)
        (self.base / 'load.json').write_bytes(json.dumps(rec).encode())
        (self.base / 'rows.jsonl').write_bytes(raw)
        bind = {'tool': 'load', 'evidence': 'load.json', 'evidence_sha256': sha256(json.dumps(rec).encode()),
                'rows': 'rows.jsonl', 'rows_sha256': sha256(raw)}
        schedule = [{'label': 'T ZERO', 'offset_seconds': 0,
                     'checks': ['Confirm block production.', 'Verify finalization.']}]
        checkpoint = cp.record_checkpoint(schedule, 0,
                                          {'Confirm block production.': {'bind': bind},
                                           'Verify finalization.': {'bind': bind}},
                                          self.base, NATIVE, ENGINE, '', '2026-10-12T14:00:03Z')
        self.assertEqual(checkpoint['status'], cp.PASS)
        run = cp.build_run('R07', schedule, 'c', NATIVE, ENGINE, [checkpoint])
        self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE), cp.PASS)

    def test_the_real_r07_schedule_matches_itself(self):
        rows = cp.schedule('R07')
        self.assertEqual(cp.schedule_sha256(rows), cp.schedule_sha256(cp.schedule('R07')))
        self.assertEqual(len(rows), 7)



class RunSchedules(unittest.TestCase):
    """R01 to R06 (P01, 14 October 2026): one T-1h checkpoint with both T-6h's
    and T-1h's checks, R07's T0 to T+24h, and each run's events."""

    def test_one_checkpoint_before_genesis_then_r07_s_then_the_events(self):
        r07 = cp.schedule('R07')
        before = [c for row in r07 if row['offset_seconds'] < 0 for c in row['checks']]
        after = [row for row in r07 if row['offset_seconds'] >= 0]
        for run, events in cp.RUN_EVENTS.items():
            with self.subTest(run=run):
                rows = cp.schedule(run)
                self.assertEqual(rows[0]['label'], 'T MINUS 1 HOUR')
                self.assertEqual(rows[0]['offset_seconds'], -3600)
                self.assertEqual(rows[0]['checks'], list(dict.fromkeys(before)))
                self.assertEqual(rows[1:1 + len(after)], after)
                tail = rows[1 + len(after):]
                self.assertEqual([row['event']['name'] for row in tail], [e[0] for e in events])
                self.assertTrue(all(row['checks'] == cp.EVENT_CHECKS for row in tail))
                self.assertNotEqual(cp.schedule_sha256(rows), cp.schedule_sha256(r07))
        self.assertEqual([len(cp.RUN_EVENTS[r]) for r in ('R01', 'R02', 'R03', 'R04', 'R05', 'R06')],
                         [0, 6, 3, 1, 0, 2])
        with self.assertRaises(cp.Failed):
            cp.schedule('R08')

    def test_a_fixed_window_ignores_the_events_after_it(self):
        rows = cp.schedule('R02')
        last = max(i for i, row in enumerate(rows) if 'event' not in row)
        self.assertEqual(rows[last]['label'], 'T PLUS 24 HOURS')
        self.assertIsNone(cp.window_of(rows, last, '2026-10-12T14:00:00Z')[1])


def write_event(base, tool, body, name):
    data = json.dumps(body).encode()
    (base / name).write_bytes(data)
    bind = {'tool': tool, 'evidence': name, 'evidence_sha256': sha256(data)}
    if tool == 'load':
        (base / 'event-rows.jsonl').write_bytes(b'')
        bind.update(rows='event-rows.jsonl', rows_sha256=sha256(b''))
    return bind


class EventCheckpoints(unittest.TestCase):
    """An event checkpoint: the reconciliation within 30 minutes of the end the
    event's own evidence records. The event tools' verify is stubbed."""

    def setUp(self):
        self.base = Path(tempfile.mkdtemp())
        self.schedule = cp.schedule('R02')
        index = {row['label']: i for i, row in enumerate(self.schedule)}
        self.host = index['AFTER THE VALIDATOR HOST RECOVERED']
        self.network = index['AFTER THE NETWORK RESTORED']
        self.restart = index['AFTER THE VALIDATOR PROCESS RESTARTED']
        for patch in (mock.patch.object(cp.recovery_tool, 'verify', return_value=cp.PASS),
                      mock.patch.object(cp.fault_tool, 'verify', return_value=cp.PASS),
                      mock.patch.object(cp.reconcile, 'verify', return_value=cp.PASS)):
            patch.start()
            self.addCleanup(patch.stop)

    def recovery_bind(self, at='2026-10-12T16:00:00+00:00'):
        moment = cp.instant(at).timestamp()
        return write_event(self.base, 'recovery', {'observations': {'instants': {'first_block_after_halt_at': moment}}},
                           'recovery.json')

    def record(self, index, event_bind, finished, actual=None):
        readings = {check: {'bind': write_bind(self.base, 'reconcile', finished)} for check in cp.EVENT_CHECKS}
        return cp.record_checkpoint(self.schedule, index, readings, self.base, NATIVE, ENGINE, '',
                                    actual or finished, event_bind=event_bind)

    def test_a_reconciliation_within_30_minutes_of_the_recovery_passes(self):
        checkpoint = self.record(self.host, self.recovery_bind(), '2026-10-12T16:10:00Z')
        self.assertEqual(checkpoint['status'], cp.PASS, checkpoint)
        self.assertEqual(checkpoint['window'], ['2026-10-12T16:00:00+00:00', '2026-10-12T16:30:00+00:00'])
        self.assertEqual(checkpoint['event']['end'], '2026-10-12T16:00:00+00:00')

    def test_a_late_reconciliation_or_an_event_outside_the_run_does_not_count(self):
        late = self.record(self.host, self.recovery_bind(), '2026-10-12T16:31:00Z', actual='2026-10-12T16:29:00Z')
        self.assertEqual(late['status'], cp.INCONCLUSIVE)
        early = self.record(self.host, self.recovery_bind('2026-10-12T13:00:00+00:00'), '2026-10-12T13:10:00Z')
        self.assertEqual(early['status'], cp.INCONCLUSIVE)
        self.assertIn('outside the run', early['event']['detail'])
        after = self.record(self.host, self.recovery_bind('2026-10-13T14:00:01+00:00'), '2026-10-13T14:10:00Z')
        self.assertEqual(after['status'], cp.INCONCLUSIVE)

    def test_a_fault_s_recovery_is_read_from_its_scenario(self):
        bind = write_event(self.base, 'faults', {'scenarios': [
            {'name': 'latency', 'recovered_at': '2026-10-12T15:00:00+00:00'},
            {'name': 'partition', 'recovered_at': '2026-10-12T15:40:00+00:00'}]}, 'faults.json')
        checkpoint = self.record(self.network, bind, '2026-10-12T15:45:00Z')
        self.assertEqual((checkpoint['status'], checkpoint['event']['end']), (cp.PASS, '2026-10-12T15:40:00+00:00'))

    def test_an_event_with_no_tool_yet_waits_and_never_passes(self):
        checkpoint = self.record(self.restart, None, '2026-10-12T15:00:00Z')
        self.assertEqual(checkpoint['status'], cp.INCONCLUSIVE)
        self.assertIn('awaits', checkpoint['event']['detail'])
        self.assertEqual(checkpoint['window'], [None, None])

    def test_another_tool_s_evidence_is_refused(self):
        bind = write_event(self.base, 'faults', {'scenarios': []}, 'faults.json')
        with self.assertRaisesRegex(cp.Failed, 'not recovery'):
            self.record(self.host, bind, '2026-10-12T16:10:00Z')

    def test_verify_re_derives_the_event(self):
        checkpoints, prev = [], ''
        for index in range(len(self.schedule)):
            if index == self.host:
                row = self.record(index, self.recovery_bind(), '2026-10-12T16:10:00Z')
                row = cp.build_checkpoint(self.schedule, index, row['actual_time'], row['checks'], prev,
                                          '2026-10-12T14:00:00Z', row['event'])
            else:
                planned = self.schedule[index]
                actual = None if 'event' in planned else (
                    cp.instant('2026-10-12T14:00:00Z') + cp.timedelta(seconds=planned['offset_seconds'] + 5)).isoformat()
                row = cp.record_checkpoint(self.schedule, index, {}, self.base, NATIVE, ENGINE, prev,
                                           actual or '2026-10-12T15:00:00Z')
            checkpoints.append(row)
            prev = row['sha256']
        run = cp.build_run('R02', self.schedule, 'dytallix-staging-1', NATIVE, ENGINE, checkpoints)
        self.assertEqual(cp.verify(run, self.base, NATIVE, ENGINE, qualifying='R02'), cp.INCONCLUSIVE)
        with self.assertRaisesRegex(cp.Failed, 'not the R07 schedule'):
            cp.verify(run, self.base, NATIVE, ENGINE, qualifying='R07')
        # Another event end, re-digested: the evidence decides, not the record.
        forged = json.loads(json.dumps(run))
        forged['checkpoints'][self.host]['event']['end'] = '2026-10-12T16:20:00+00:00'
        body = {k: v for k, v in forged.items() if k != 'sha256'}
        forged['sha256'] = cp.digest(cp.canonical(body))
        with self.assertRaisesRegex(cp.Failed, 'does not follow from its evidence'):
            cp.verify(forged, self.base, NATIVE, ENGINE)
        # R05 has R01's schedule; the run name tells them apart.
        r01 = cp.build_run('R05', cp.schedule('R01'), 'c', NATIVE, ENGINE, [])
        with self.assertRaisesRegex(cp.Failed, 'not the R01 schedule'):
            cp.verify(r01, self.base, NATIVE, ENGINE, qualifying='R01')


if __name__ == '__main__':
    unittest.main()
