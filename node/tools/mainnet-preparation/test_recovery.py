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


class Clock:
    def __init__(self, now=1_800_000_000.0):
        self.now = now

    def __call__(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class Client:
    def __init__(self, *views):
        self.views = list(views)

    def status(self):
        return {'effective_validators': self.views.pop(0)}


def operator_status(height):
    return json.dumps({'result': {'sync_info': {'latest_block_height': str(height),
                                                'latest_block_hash': f'B{height}', 'latest_app_hash': f'A{height}'}}})


class AgentChannel:
    """One host's agent: reconcile-read answers with these heights."""
    every = 2

    def __init__(self, host, heights, status='DONE'):
        self.host, self.heights, self.status = host, heights, status
        self.sent = []

    def send(self, command):
        self.sent.append(command)
        return 100 + len(self.sent)

    def wait(self, sequences, timeout):
        steps = [{'argv': ['curl', 'status'], 'exit': 0, 'stdout': operator_status(h)} for h in self.heights]
        steps.append({'argv': ['curl', 'status'], 'exit': 7, 'stdout': ''})
        return [{'status': self.status, 'steps': steps, 'command_sha256': 'c', 'finished_at': 'f'}]


SET = [{'validator_id': 'v1', 'power': '100'}]


class RealHostRunTests(unittest.TestCase):
    def setUp(self):
        import tempfile
        self.directory = tempfile.TemporaryDirectory()
        self.run = __import__('pathlib').Path(self.directory.name) / 'run'
        self.clock = Clock()

    def tearDown(self):
        self.directory.cleanup()

    def pages(self, *heights):
        found = iter(heights)

        def read():
            height = next(found)
            return {'http': None, 'refused': True} if height is None else {'http': 200, 'height': height,
                                                                             'time': 't', 'read_at': 'r'}
        return read

    def step(self, name):
        self.clock.sleep(60)
        rv.mark(self.run, name, clock=self.clock, moved_to='/x.f17-1' if name == 'wal_set_aside' else None)

    def until_first_block(self, halt=(40, 40), after=(None, 40, 41)):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        self.assertEqual(rv.halt(self.run, self.pages(*halt), sleep=self.clock.sleep), list(halt))
        for name in rv.MARKS[1:]:
            self.step(name)
        return rv.first_block(self.run, self.pages(*after), clock=self.clock, sleep=self.clock.sleep)

    def finish(self, validator, sentry, after=SET, verify_exit=0):
        channels = {'validator': AgentChannel('validator-1', validator), 'sentry': AgentChannel('sentry-1', sentry)}
        record = rv.finish(self.run, channels, Client(after), 'c', GENESIS, verify_exit, TARGET, say=lambda _: None,
                           wall=self.clock, sleep=self.clock.sleep)
        return record, channels

    def test_a_run_by_the_procedure_passes_and_verifies(self):
        self.assertEqual(self.until_first_block(), 41)
        record, channels = self.finish([43, 44, 45], [44, 45, 46])
        self.assertEqual(record['status'], rv.PASS, record['checks'])
        self.assertEqual(rv.verify(json.loads(json.dumps(record)), GENESIS, TARGET), rv.PASS)
        obs = record['observations']
        self.assertEqual(obs['clock'], rv.CLOCK)
        self.assertEqual(obs['compared']['validator'], {'height': 45, 'block_hash': 'b45', 'app_hash': 'a45'})
        self.assertEqual(obs['reached'], {'validator': 45, 'sentry': 46})
        self.assertEqual(obs['wal_set_aside'], {'host': 'sentry', 'moved_to': '/x.f17-1'})
        self.assertEqual(obs['first_block']['height'], 41)
        self.assertEqual([c.sent for c in channels.values()], [['reconcile-read'], ['reconcile-read']])
        # Every instant on the founder's clock, in the procedure's order.
        instants = obs['instants']
        self.assertEqual(sorted(instants, key=instants.get), list(rv.STEPS))

    def test_a_step_marked_out_of_order_is_kept_and_fails_the_order(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('installed')
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        for name in ('wal_set_aside', 'sentry_started', 'validator_started'):
            self.step(name)
        rv.first_block(self.run, self.pages(41), clock=self.clock, sleep=self.clock.sleep)
        record, _ = self.finish([44], [44])
        rows = {r['check']: r['status'] for r in record['checks']}
        self.assertEqual(rows['order'], rv.FAIL)
        self.assertEqual(record['status'], rv.FAIL)

    def test_each_step_once_and_the_halt_after_the_fence(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        with self.assertRaises(rv.Failed):
            rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        with self.assertRaises(rv.Failed):
            rv.first_block(self.run, self.pages(41), clock=self.clock, sleep=self.clock.sleep)
        self.step('fenced')
        with self.assertRaises(rv.Failed):
            self.step('fenced')
        with self.assertRaises(rv.Failed):
            rv.mark(self.run, 'started', clock=self.clock)
        with self.assertRaises(rv.Failed):
            rv.start(self.run, Client(SET), clock=self.clock)

    def test_no_validator_listed_is_no_declaration(self):
        with self.assertRaises(rv.Failed):
            rv.start(self.run, Client([]), clock=self.clock)
        with self.assertRaises(rv.Failed):
            rv.load_run(self.run)

    def test_the_halt_is_read_a_minute_apart(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        before = self.clock.now
        rv.halt(self.run, self.pages(40, 41), sleep=self.clock.sleep)
        self.assertEqual(self.clock.now - before, rv.HALT_GAP)
        self.assertEqual(rv.load_run(self.run)['observations']['halt_readings'], [40, 41])

    def test_a_sentry_still_moving_after_the_fence_fails(self):
        self.until_first_block(halt=(40, 41), after=(42,))
        record, _ = self.finish([44, 45], [44, 45])
        self.assertEqual({r['check']: r['status'] for r in record['checks']}['halt'], rv.FAIL)

    def test_a_halt_page_without_a_height_is_read_again_before_the_votes_are_set_aside(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        self.assertEqual(rv.halt(self.run, self.pages(40, None), sleep=self.clock.sleep), [40, None])
        self.assertEqual(rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep), [40, 40])
        obs = rv.load_run(self.run)['observations']
        self.assertEqual(len(obs['halt_unread']), 1)
        with self.assertRaises(rv.Failed):
            rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        for name in rv.MARKS[1:]:
            self.step(name)
        self.assertEqual(rv.first_block(self.run, self.pages(41), clock=self.clock, sleep=self.clock.sleep), 41)

    def test_no_halt_height_once_the_votes_are_set_aside(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, None), sleep=self.clock.sleep)
        self.step('wal_set_aside')
        with self.assertRaises(rv.Failed):
            rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        for name in rv.MARKS[2:]:
            self.step(name)
        with self.assertRaises(rv.Failed):
            rv.first_block(self.run, self.pages(41), clock=self.clock, sleep=self.clock.sleep)

    def test_no_first_block_within_the_timeout_fails(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        for name in rv.MARKS[1:]:
            self.step(name)
        with self.assertRaises(rv.Failed):
            rv.first_block(self.run, lambda: {'http': 503, 'height': 40}, clock=self.clock,
                           sleep=self.clock.sleep, timeout=30)

    def test_the_reader_gap_is_kept(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        for name in rv.MARKS[1:]:
            self.step(name)
        answers = iter([{'http': 200, 'height': 40}, {}, {}, {'http': 200, 'height': 41}])
        rv.first_block(self.run, lambda: next(answers), clock=self.clock, sleep=lambda s: self.clock.sleep(5))
        self.assertEqual(rv.load_run(self.run)['observations']['first_block']['reader_gap_seconds'], 15.0)

    def test_no_common_height_past_the_halt_is_inconclusive_and_read_again(self):
        self.until_first_block()
        record, _ = self.finish([41, 42], [42, 43])
        rows = {r['check']: r['status'] for r in record['checks']}
        self.assertEqual(rows['state'], rv.INCONCLUSIVE)
        self.assertNotIn('compared', record['observations'])
        record, _ = self.finish([43, 44], [44, 45])
        self.assertEqual(record['status'], rv.PASS, record['checks'])

    def test_a_changed_set_or_a_failed_verify_fails(self):
        self.until_first_block()
        record, _ = self.finish([44], [44], after=[{'validator_id': 'v2', 'power': '100'}])
        self.assertEqual({r['check']: r['status'] for r in record['checks']}['validator_set'], rv.FAIL)
        record, _ = self.finish([44], [44], verify_exit=1)
        self.assertEqual({r['check']: r['status'] for r in record['checks']}['host_verify'], rv.FAIL)

    def test_an_unanswered_agent_reads_nothing(self):
        self.until_first_block()
        channels = {'validator': AgentChannel('validator-1', [44], status='EXPIRED'),
                    'sentry': AgentChannel('sentry-1', [44])}
        record = rv.finish(self.run, channels, Client(SET), 'c', GENESIS, 0, TARGET, say=lambda _: None,
                           wall=self.clock, sleep=self.clock.sleep)
        self.assertEqual(record['observations']['reached'], {'validator': None, 'sentry': 44})
        self.assertEqual(record['status'], rv.INCONCLUSIVE)

    def test_the_commands_are_wired(self):
        import subprocess
        import sys
        out = subprocess.run([sys.executable, rv.__file__, '--help'], capture_output=True, text=True).stdout
        for command in ('verify', 'start', 'mark', 'halt', 'first-block', 'finish'):
            self.assertIn(command, out)


class StagingTests(unittest.TestCase):
    def test_the_drill_is_wired(self):
        import staging_network
        self.assertTrue(callable(staging_network.validator_recovery_drill))
        self.assertIs(staging_network.OPERATOR_STATUS, __import__('reconcile').OPERATOR_STATUS)


if __name__ == '__main__':
    unittest.main()
