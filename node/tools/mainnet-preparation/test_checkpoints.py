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


if __name__ == '__main__':
    unittest.main()
