"""The fault injector (simulation plan, T5): how a scenario's status follows
from its observations, that a no-op fault cannot pass, the recovery-target
rule, and its digest-bound evidence. The live injection through the agent and
the sentry's catch-up are exercised by the Host install workflow's fault step;
here the observations are synthetic so the tests cover T5's logic alone."""
import hashlib
import json
import unittest

import faults

ENGINE = json.dumps({'genesis_time': '2026-10-12T14:00:00Z', 'chain_id': 'dytallix-staging-1'}).encode()
NATIVE = json.dumps({'accounts': [{'address': 'a', 'balances': {'udrt': '1000', 'udgt': '600'}}]}).encode()


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def scenario(name='partition', **over):
    """A clean, recovered scenario; override fields to make it fail."""
    base = {
        'name': name,
        'command': {'command': f'fault-{name}' if name != 'process_kill' else 'fault-kill',
                    'args': {}, 'status': 'DONE'},
        'clear': {'command': 'fault-clear', 'args': {}, 'status': 'DONE'},
        'baseline_height': 100,
        'effect': {'installed': True} if name == 'partition'
        else {'qdisc': 'netem delay 200ms'} if name in ('latency', 'loss')
        else {'active': 'inactive'},
        'height_at_clear': 104,
        'injected_at': '2026-10-12T14:05:00Z',
        'cleared_at': '2026-10-12T14:05:30Z',
        'recovered_at': '2026-10-12T14:06:00Z',
        'recovered_height': 110,
    }
    base.update(over)
    return base


class Effect(unittest.TestCase):
    def test_netem_effect_needs_a_netem_qdisc(self):
        self.assertTrue(faults.effect_holds('netem', {'qdisc': 'netem delay 200ms'}))
        self.assertFalse(faults.effect_holds('netem', {'qdisc': 'pfifo_fast'}))
        self.assertFalse(faults.effect_holds('netem', {}))

    def test_partition_effect_needs_the_drop_installed(self):
        self.assertTrue(faults.effect_holds('partition', {'installed': True}))
        self.assertFalse(faults.effect_holds('partition', {'installed': False}))
        self.assertFalse(faults.effect_holds('partition', {}))

    def test_peers_effect_needs_closed_connections_that_stay_closed(self):
        closed = ['10.77.0.11:26656 10.77.0.1:41234']
        self.assertTrue(faults.effect_holds('peers', {'closed': closed, 'after': ['10.77.0.11:50130 10.77.0.1:26656']}))
        self.assertTrue(faults.effect_holds('peers', {'closed': closed, 'after': []}))
        self.assertFalse(faults.effect_holds('peers', {'closed': [], 'after': []}), 'nothing closed: a no-op')
        self.assertFalse(faults.effect_holds('peers', {'closed': closed, 'after': closed}), 'still open')

    def test_the_peer_drop_proof_reads_both_listings(self):
        result = {'steps': [{'stdout': '0 0 10.77.0.11:26656 10.77.0.1:41234\n'},
                            {'stdout': 'ESTAB 0 0 10.77.0.11:50130 10.77.0.1:26656 users:((x))\n'}]}
        self.assertEqual(faults.effect_of('peer_drop', result),
                         {'closed': ['10.77.0.11:26656 10.77.0.1:41234'], 'after': ['10.77.0.11:50130 10.77.0.1:26656']})

    def test_process_effect_needs_a_down_node(self):
        self.assertTrue(faults.effect_holds('process', {'active': 'inactive'}))
        self.assertFalse(faults.effect_holds('process', {'active': 'active'}))


class Order(unittest.TestCase):
    def test_the_peer_drop_runs_before_the_faults_that_break_the_peers(self):
        names = [s['name'] for s in faults.SCENARIOS]
        self.assertLess(names.index('peer_drop'), min(names.index(n) for n in faults.BREAKS_PEERS))

    def test_a_peer_drop_waits_for_the_redial_after_a_partition_or_a_kill(self):
        slept, said = [], []
        self.assertEqual(faults.settle(None, clock=lambda: 500.0, sleep=slept.append, say=said.append), 0.0)
        self.assertEqual(faults.settle(100.0, clock=lambda: 200.0, sleep=slept.append, say=said.append), 0.0)
        self.assertEqual(faults.settle(100.0, clock=lambda: 120.0, sleep=slept.append, say=said.append),
                         faults.REDIAL_SECONDS - 20)
        self.assertEqual(slept, [faults.REDIAL_SECONDS - 20])
        self.assertIn('redial before the peer drop', said[0])


class Evaluate(unittest.TestCase):
    def status(self, scn, target=None):
        return faults.evaluate(scn, target)[0]

    def test_recovered_scenario_without_target_is_inconclusive(self):
        self.assertEqual(self.status(scenario()), faults.INCONCLUSIVE)

    def test_within_target_passes_over_target_fails(self):
        self.assertEqual(self.status(scenario(), target=60), faults.PASS)
        self.assertEqual(self.status(scenario(), target=10), faults.FAIL)

    def test_a_noop_fault_cannot_pass(self):
        self.assertEqual(self.status(scenario(effect={}), target=60), faults.FAIL)
        self.assertEqual(self.status(scenario('latency', effect={'qdisc': 'pfifo_fast'}), target=60), faults.FAIL)

    def test_a_fault_that_did_not_run_fails(self):
        self.assertEqual(self.status(scenario(command={'status': 'FAILED'}), target=60), faults.FAIL)

    def test_lost_finalized_history_fails(self):
        self.assertEqual(self.status(scenario(recovered_height=99), target=60), faults.FAIL)

    def test_no_advance_after_clear_fails(self):
        self.assertEqual(self.status(scenario(recovered_height=104), target=60), faults.FAIL)

    def test_never_recovered_fails(self):
        self.assertEqual(self.status(scenario(recovered_at=None, recovered_height=None), target=60), faults.FAIL)

    def test_an_unregistered_scenario_is_refused(self):
        with self.assertRaises(faults.Failed):
            faults.evaluate(scenario('meteor'), None)


class Evidence(unittest.TestCase):
    def build(self, scenarios, target=None):
        return faults.build_run('dytallix-staging-1', NATIVE, ENGINE, scenarios, target, 'sentry-1')

    def test_build_then_verify_roundtrips(self):
        record = self.build([scenario('latency'), scenario('partition')])
        self.assertEqual(record['status'], faults.INCONCLUSIVE)
        self.assertEqual(faults.verify(record, NATIVE, ENGINE), faults.INCONCLUSIVE)

    def test_run_fails_if_any_scenario_fails(self):
        record = self.build([scenario('latency'), scenario('partition', recovered_height=1)], target=60)
        self.assertEqual(record['status'], faults.FAIL)
        self.assertEqual(faults.verify(record, NATIVE, ENGINE), faults.FAIL)

    def test_run_passes_when_all_pass(self):
        record = self.build([scenario('latency'), scenario('process_kill')], target=60)
        self.assertEqual(record['status'], faults.PASS)
        self.assertEqual(faults.verify(record, NATIVE, ENGINE), faults.PASS)

    def test_a_tampered_digest_is_caught(self):
        record = self.build([scenario()])
        record['scenarios'][0]['recovered_height'] = 999
        with self.assertRaises(faults.Failed):
            faults.verify(record, NATIVE, ENGINE)

    def test_an_edited_target_is_caught(self):
        record = self.build([scenario()], target=60)
        record['recovery_target_seconds'] = 100000
        with self.assertRaises(faults.Failed):
            faults.verify(record, NATIVE, ENGINE)

    def test_a_swapped_genesis_is_caught(self):
        record = self.build([scenario()])
        with self.assertRaises(faults.Failed):
            faults.verify(record, NATIVE, json.dumps({'genesis_time': 'x', 'chain_id': 'other'}).encode())

    def test_a_softened_kind_is_caught(self):
        record = self.build([scenario('partition')])
        record['scenarios'][0]['kind'] = 'netem'
        record['sha256'] = faults.digest(faults.canonical({k: v for k, v in record.items() if k != 'sha256'}))
        with self.assertRaises(faults.Failed):
            faults.verify(record, NATIVE, ENGINE)

    def test_an_empty_run_is_refused(self):
        with self.assertRaises(faults.Failed):
            faults.verify(self.build([]), NATIVE, ENGINE)

    def test_a_non_positive_target_is_refused(self):
        record = self.build([scenario()], target=60)
        record['recovery_target_seconds'] = 0
        record['sha256'] = faults.digest(faults.canonical({k: v for k, v in record.items() if k != 'sha256'}))
        with self.assertRaises(faults.Failed):
            faults.verify(record, NATIVE, ENGINE)


if __name__ == '__main__':
    unittest.main()
