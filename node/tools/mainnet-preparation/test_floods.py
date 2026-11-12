"""The flood family (T4; P01, 9 November 2026): how each attempt's outcome
follows from its observations, that a flood that met no full queue proves
nothing, and the digest-bound record. The live floods through the client
channel are exercised by the Host install workflow's flood step; here the
observations are synthetic."""
import base64
import json
import threading
import unittest
from unittest import mock

import floods
import load

NATIVE, ENGINE = b'{"native": 1}', b'{"engine": 1}'


def tx(**over):
    seen = {'bound': 64, 'sent': 80, 'heads': [120], 'admitted': 64, 'committed': 64, 'lost': 0,
            'queue_refused': 16, 'other_refusals': []}
    seen.update(over)
    return floods.finish('transaction_flood', 0, seen)


def req(**over):
    seen = {'flooders': 7, 'flood_served': 900, 'flood_errors': 3, 'honest_reads': 16, 'honest_served': 16,
            'honest_heights': [120, 123]}
    seen.update(over)
    return floods.finish('request_flood', 0, seen)


def controls():
    return [{'type': 'control', 'committed': True, 'height': 140, 'code': 0},
            {'type': 'control', 'committed': True, 'height': 142, 'code': 0}]


class Classify(unittest.TestCase):
    def test_a_full_queue_that_refuses_the_excess_and_commits_the_rest_holds(self):
        self.assertEqual(tx()['outcome'], 'refused')

    def test_an_evicted_or_dropped_admission_is_a_success(self):
        self.assertEqual(tx(committed=63, lost=1)['outcome'], 'admitted')

    def test_more_admitted_than_the_bound_at_one_head_is_a_success(self):
        self.assertEqual(tx(admitted=80, committed=80, queue_refused=0)['outcome'], 'admitted')

    def test_a_block_between_the_batches_without_a_full_queue_is_made_again(self):
        self.assertEqual(tx(heads=[120, 121], admitted=80, committed=80, queue_refused=0)['outcome'], 'transport')
        # A block in between that still met a full queue counts.
        self.assertEqual(tx(heads=[120, 121], admitted=70, committed=70, queue_refused=10)['outcome'], 'refused')

    def test_another_refusal_reason_fails_the_type(self):
        self.assertEqual(tx(other_refusals=['Ordinary spending nonce is stale'])['outcome'], 'refused_other')

    def test_a_transport_error_is_made_again(self):
        self.assertEqual(tx(error='the gateway: timed out')['outcome'], 'transport')

    def test_an_honest_client_served_every_time_while_the_chain_advances_holds(self):
        self.assertEqual(req()['outcome'], 'refused')

    def test_an_honest_read_not_served_or_a_stalled_chain_is_a_success(self):
        self.assertEqual(req(honest_served=15)['outcome'], 'admitted')
        self.assertEqual(req(honest_heights=[120, 120])['outcome'], 'admitted')
        self.assertEqual(req(honest_reads=0, honest_served=0)['outcome'], 'admitted')

    def test_the_diagnostics_do_not_change_the_judgement(self):
        seen = dict(honest_slowest_s=4.2, honest_misses=[], flood_first_errors=['the gateway: timed out'],
                    runner_load=[3.1, 2.0, 1.5])
        self.assertEqual(req(**seen)['outcome'], 'refused')
        missed = dict(seen, honest_served=15, honest_misses=[{'after_s': 45.0, 'error': 'the gateway: timed out'}])
        self.assertEqual(req(**missed)['outcome'], 'admitted')

    def test_a_flood_that_sent_nothing_is_made_again(self):
        self.assertEqual(req(flood_served=0)['outcome'], 'transport')

    def test_the_observed_counts_follow_from_the_rows(self):
        def row(status, log=None, head=120):
            return {'status': status, 'log': log, 'submit_height': head}
        rows = ([row('committed')] * 3 + [row('lost')] + [row('refused', floods.QUEUE_REFUSAL)] * 2
                + [row('refused', 'Ordinary spending nonce is stale', 121)])
        seen = floods.transaction_observed(rows, 3)
        self.assertEqual((seen['admitted'], seen['committed'], seen['lost'], seen['queue_refused'], seen['heads']),
                         (4, 3, 1, 2, [120, 121]))
        self.assertEqual(seen['other_refusals'], ['Ordinary spending nonce is stale'])

    def test_a_block_partway_through_a_batch_is_not_an_over_admission(self):
        # 11 November 2026: every batch read head 1060 before its sends, a
        # block came partway through one, and 67 were admitted against 64.
        def row(status, log=None):
            return {'status': status, 'log': log, 'submit_height': 1060}
        rows = [row('committed')] * 67 + [row('refused', floods.QUEUE_REFUSAL)] * 13
        unchanged = floods.transaction_observed(rows, 64, [1060])
        self.assertEqual(floods.classify({'type': 'transaction_flood', 'observed': unchanged}), 'admitted')
        moved = floods.transaction_observed(rows, 64, [1061])
        self.assertEqual(moved['heads'], [1060, 1061])
        self.assertEqual(floods.classify({'type': 'transaction_flood', 'observed': moved}), 'refused')
        self.assertEqual(floods.transaction_observed(rows, 64, [None])['heads'], [1060])


class Record(unittest.TestCase):
    def rows(self, n=floods.TARGET):
        return [dict(tx(), attempt=k) for k in range(n)] + [dict(req(), attempt=k) for k in range(n)] + controls()

    def record(self, rows, bound=64):
        return floods.build_record('dytallix-staging-1', NATIVE, ENGINE, rows, floods.TARGET, bound,
                                   '2026-11-09T20:00:00Z', '2026-11-09T20:30:00Z')

    def test_ten_of_each_with_committed_controls_pass_and_verify(self):
        rows = self.rows()
        record = self.record(rows)
        self.assertEqual(record['status'], floods.PASS, record['checks'])
        self.assertEqual(record['limits']['queue_max_entries'], 64)
        self.assertEqual(floods.verify(json.loads(json.dumps(record)), floods.rows_bytes(rows), NATIVE, ENGINE),
                         floods.PASS)

    def test_fewer_than_ten_or_one_success_fails(self):
        self.assertEqual(self.record(self.rows(9))['status'], floods.FAIL)
        rows = self.rows() + [tx(committed=63, lost=1)]
        record = self.record(rows)
        self.assertEqual((record['status'], record['successes']), (floods.FAIL, 1))

    def test_a_control_that_did_not_commit_or_a_head_that_did_not_rise_fails(self):
        rows = self.rows()
        rows[-1] = dict(rows[-1], committed=False)
        self.assertFalse(self.record(rows)['checks']['controls'])
        rows = self.rows()
        rows[-1] = dict(rows[-1], height=140)
        self.assertFalse(self.record(rows)['checks']['advancing'])

    def test_edits_show(self):
        rows = self.rows()
        record = self.record(rows)
        raw = floods.rows_bytes(rows)
        with self.assertRaises(floods.Failed):
            floods.verify(dict(record, status=floods.FAIL), raw, NATIVE, ENGINE)
        with self.assertRaises(floods.Failed):
            floods.verify(record, raw.replace(b'"lost":0', b'"lost":1', 1), NATIVE, ENGINE)
        with self.assertRaises(floods.Failed):
            floods.verify(record, raw, b'{"other": 1}', ENGINE)
        edited = dict(record, limits=dict(record['limits'], flooders=8))
        edited['sha256'] = floods.digest(floods.canonical({k: v for k, v in edited.items() if k != 'sha256'}))
        with self.assertRaises(floods.Failed):
            floods.verify(edited, raw, NATIVE, ENGINE)

    def test_a_row_judged_against_another_bound_is_refused(self):
        rows = self.rows()
        rows[0] = floods.finish('transaction_flood', 0, dict(rows[0]['observed'], bound=2000))
        record = self.record(rows)
        with self.assertRaises(floods.Failed):
            floods.verify(record, floods.rows_bytes(rows), NATIVE, ENGINE)

    def test_a_tampered_outcome_is_refused_when_built(self):
        rows = self.rows()
        rows[0] = dict(rows[0], outcome='refused', observed=dict(rows[0]['observed'], lost=2))
        with self.assertRaises(floods.Failed):
            self.record(rows)

    def test_the_request_flood_leaves_the_honest_address_its_connections(self):
        self.assertLessEqual(floods.FLOODERS * floods.CHANNEL['channel_per_address'],
                             floods.CHANNEL['channel_total'] - floods.CHANNEL['channel_per_address'])


# The CI path with stand-ins for the chain and the flooders: what the runner
# observes stays as it was before the load-machine mode.

class FakeTracker:
    def __init__(self):
        self.rows = []

    def add(self, row):
        self.rows.append(row)

    def drain(self, timeout, interval=1.0):
        for row in self.rows:
            row['status'] = 'committed'
        self.rows = []


class FakeRun:
    def __init__(self):
        self.s = load.Settings(signers=2, block_time=1.0, expiry_blocks=2, poll_interval=0.0)
        self.tracker = FakeTracker()
        self.calls = []

    def refresh_profile(self):
        self.calls.append('profile')

    def sync(self, wallets):
        self.calls.append(('sync', len(wallets)))

    def sign(self, row, wallet, actions):
        row.update(id=wallet.label, nonce=0)
        return row, wallet.label.encode()


class QueueRpc:
    """A sentry whose queue takes `bound` transactions; the head is read
    from `heads` in turn and the batches meet the last."""

    def __init__(self, bound, heads=(120, 121), connections=2):
        self.bound, self.heads, self.connections = bound, list(heads), connections
        self.queued, self.lock, self.reads = 0, threading.Lock(), 0

    def one(self, label, method, params):
        with self.lock:
            self.reads += 1
            head = self.heads[min(self.reads, len(self.heads)) - 1]
        return {'response': {'last_block_height': str(head)}}

    def timed(self, label, requests):
        out = [{'response': {'last_block_height': str(self.heads[-1])}}]
        with self.lock:
            for _, params in requests[1:]:
                tx = base64.b64decode(params['tx'])
                if self.queued < self.bound:
                    self.queued += 1
                    out.append({'code': 0, 'hash': load.sha256(tx)})
                else:
                    out.append({'code': 20, 'log': floods.QUEUE_REFUSAL})
        return out, 1.0


def flood_wallets(n, start=0):
    return [load.Wallet(f'load-{k:04}', 'load', f'dyt1{k}', f'{k:064x}') for k in range(start, start + n)]


FUNDER = load.Wallet('funder', 'funder', 'dyt1f', 'f' * 64)


class CiPath(unittest.TestCase):
    def test_the_transaction_flood_observes_a_full_queue(self):
        run, rpc = FakeRun(), QueueRpc(5)
        seen = floods.transaction_flood(run, rpc, flood_wallets(21), FUNDER, 5, lambda m: None)
        self.assertEqual(seen, {'bound': 5, 'sent': 21, 'heads': [121], 'admitted': 5, 'committed': 5, 'lost': 0,
                                'queue_refused': 16, 'other_refusals': []})
        self.assertEqual(run.calls, ['profile', ('sync', 21)])
        self.assertEqual(floods.finish('transaction_flood', 0, seen)['outcome'], 'refused')

    def test_the_request_flood_observes_the_honest_reads_and_the_flooders(self):
        clock = [0.0]

        class Time:
            @staticmethod
            def monotonic():
                return clock[0]

            @staticmethod
            def sleep(s):
                clock[0] += s

        class Flooder:
            def communicate(self, timeout):
                return json.dumps({'served': 100, 'errors': 1, 'first_errors': ['busy']}) + '\n', ''

        class Honest:
            def __init__(self):
                self.n = 0

            def one(self, label, method, params):
                self.n += 1
                clock[0] += 0.5
                if self.n == 2:
                    raise load.Transport('the gateway: timed out')
                return {'response': {'last_block_height': str(100 + self.n)}}

        started = []

        def popen(argv, **kwargs):
            started.append(argv)
            return Flooder()
        with mock.patch.object(floods, 'time', Time), \
                mock.patch.object(floods.os, 'getloadavg', return_value=(1.0, 0.5, 0.25)), \
                mock.patch.object(floods.subprocess, 'Popen', side_effect=popen):
            seen = floods.request_flood(Honest(), 'dytallix', {}, [f'ns{n}' for n in range(8)], lambda m: None,
                                        seconds=8)
        self.assertEqual(len(started), floods.FLOODERS)
        self.assertEqual(started[0][:4], ['ip', 'netns', 'exec', 'ns0'])
        self.assertEqual(seen, {'flooders': 7, 'flood_served': 700, 'flood_errors': 7, 'honest_reads': 3,
                                'honest_served': 2, 'honest_heights': [101, 103], 'honest_slowest_s': 0.5,
                                'honest_misses': [{'after_s': 0.5, 'error': 'the gateway: timed out'}],
                                'flood_first_errors': ['busy', 'busy', 'busy'], 'runner_load': [1.0, 0.5, 0.25]})


if __name__ == '__main__':
    unittest.main()
