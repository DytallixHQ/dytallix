"""The validator recovery drill's evidence (simulation plan, T9): each rule of
the fence-first procedure, what makes it PASS, FAIL or INCONCLUSIVE, and its
digest-bound record. The real run is the Host install workflow's validator
recovery drill."""
import copy
import json
import unittest

import recovery as rv

GENESIS = b'{"accounts": []}'
TARGET = 8 * 3600


def observations():
    """A recovery that followed the procedure: halted at 40, signing again."""
    instants = {step: 1_800_000_000 + 60 * i for i, step in enumerate(rv.STEPS)}
    node = {'height': 45, 'block_hash': 'b45', 'app_hash': 'a44'}
    return {'clock': 'the runner', 'instants': instants, 'halt_readings': [40, 40],
            'reached': {'validator': 45, 'sentry': 45}, 'compared': {'validator': dict(node), 'sentry': dict(node)},
            'validators_before': [{'validator_id': 'v1', 'power': '100'}],
            'validators_after': [{'validator_id': 'v1', 'power': '100'}], 'host_verify_exit': 0,
            'wal_set_aside': {'host': 'sentry-1', 'moved_to': '/x.f17-1'}}


def checks(obs, target=TARGET):
    rows, status = rv.evaluate(obs, target)
    return {r['check']: r['status'] for r in rows}, status


class EvaluateTests(unittest.TestCase):
    def test_a_recovery_by_the_procedure_passes(self):
        rows, status = checks(observations())
        self.assertEqual(status, rv.PASS, rows)
        self.assertEqual(set(rows), {'order', 'halt', 'signing', 'state', 'validator_set', 'host_verify', 'objective'})

    def test_an_install_before_the_fence_fails(self):
        obs = observations()
        obs['instants']['installed_at'] = obs['instants']['declared_at'] + 1
        rows, status = checks(obs)
        self.assertEqual((rows['order'], status), (rv.FAIL, rv.FAIL))

    def test_the_validator_started_before_the_old_votes_were_set_aside_fails(self):
        obs = observations()
        obs['instants']['validator_started_at'] = obs['instants']['fenced_at'] + 1
        self.assertEqual(checks(obs)[0]['order'], rv.FAIL)

    def test_a_sentry_still_moving_after_the_fence_fails(self):
        obs = observations()
        obs['halt_readings'] = [40, 41]
        self.assertEqual(checks(obs)[0]['halt'], rv.FAIL)

    def test_one_block_past_the_halt_is_not_signing_again(self):
        obs = observations()
        obs['reached']['validator'] = 41
        self.assertEqual(checks(obs)[0]['signing'], rv.FAIL)

    def test_differing_nodes_fail(self):
        obs = observations()
        obs['compared']['sentry']['app_hash'] = 'other'
        self.assertEqual(checks(obs)[0]['state'], rv.FAIL)

    def test_a_comparison_at_the_halt_is_inconclusive(self):
        obs = observations()
        for node in obs['compared'].values():
            node['height'] = 40
        self.assertEqual(checks(obs)[0]['state'], rv.INCONCLUSIVE)

    def test_a_changed_validator_set_fails(self):
        obs = observations()
        obs['validators_after'] = []
        self.assertEqual(checks(obs)[0]['validator_set'], rv.FAIL)

    def test_a_host_that_does_not_verify_fails(self):
        obs = observations()
        obs['host_verify_exit'] = 1
        self.assertEqual(checks(obs)[0]['host_verify'], rv.FAIL)

    def test_over_the_objective_fails(self):
        obs = observations()
        obs['instants']['first_block_after_halt_at'] = obs['instants']['declared_at'] + TARGET + 1
        self.assertEqual(checks(obs)[0]['objective'], rv.FAIL)

    def test_missing_readings_are_inconclusive_never_a_pass(self):
        for key in ('instants', 'halt_readings', 'reached', 'compared', 'validators_before', 'host_verify_exit'):
            obs = observations()
            del obs[key]
            rows, status = checks(obs)
            self.assertEqual(status, rv.INCONCLUSIVE, (key, rows))


class EvidenceTests(unittest.TestCase):
    def test_the_record_verifies_and_shows_any_edit(self):
        record = rv.build('c', GENESIS, observations(), TARGET)
        self.assertEqual(rv.verify(json.loads(json.dumps(record)), GENESIS, TARGET), rv.PASS)
        edited = copy.deepcopy(record)
        edited['observations']['halt_readings'] = [40, 41]
        with self.assertRaises(rv.Failed):
            rv.verify(edited, GENESIS, TARGET)
        with self.assertRaises(rv.Failed):
            rv.verify(record, b'{}', TARGET)

    def test_checks_that_do_not_follow_are_refused(self):
        body = rv.build('c', GENESIS, observations(), TARGET)
        body = {k: v for k, v in body.items() if k != 'sha256'}
        body['observations']['host_verify_exit'] = 1
        body['sha256'] = rv.digest(rv.canonical(body))
        with self.assertRaises(rv.Failed):
            rv.verify(body, GENESIS, TARGET)

    def test_only_the_approved_objective(self):
        self.assertEqual(rv.approved_target(), TARGET)
        lenient = rv.build('c', GENESIS, observations(), 10 * TARGET)
        with self.assertRaises(rv.Failed):
            rv.verify(lenient, GENESIS)
        self.assertEqual(rv.verify(rv.build('c', GENESIS, observations(), TARGET), GENESIS), rv.PASS)


class StagingTests(unittest.TestCase):
    def test_the_drill_is_wired(self):
        import staging_network
        self.assertTrue(callable(staging_network.validator_recovery_drill))
        self.assertIs(staging_network.OPERATOR_STATUS, __import__('reconcile').OPERATOR_STATUS)


if __name__ == '__main__':
    unittest.main()
