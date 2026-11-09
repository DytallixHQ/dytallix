"""The flood family (T4; P01, 9 November 2026): how each attempt's outcome
follows from its observations, that a flood that met no full queue proves
nothing, and the digest-bound record. The live floods through the client
channel are exercised by the Host install workflow's flood step; here the
observations are synthetic."""
import json
import unittest

import floods

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


if __name__ == '__main__':
    unittest.main()
