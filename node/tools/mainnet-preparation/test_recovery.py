"""The validator recovery drill's evidence (simulation plan, T9): each rule of
the fence-first procedure, what makes it PASS, FAIL or INCONCLUSIVE, and its
digest-bound record. The real run is the Host install workflow's validator
recovery drill."""
import copy
import json
import unittest
from pathlib import Path

import reconcile
import recovery as rv

GENESIS = b'{"accounts": []}'
# The staging sentry's process bounds: a 160 s hold.
PROCESS = {'startup_millis': 60000, 'stop_millis': 30000, 'kill_millis': 10000, 'poll_millis': 100}
TARGET = 8 * 3600


def observations():
    """A recovery that followed the procedure: halted at 40, signing again."""
    instants = {step: 1_800_000_000 + 200 * i for i, step in enumerate(rv.STEPS)}
    node = {'height': 45, 'block_hash': 'b45', 'app_hash': 'a44'}
    started = instants['sentry_started_at']
    hold = {'process': dict(PROCESS), 'from': started, 'readings': [{'at': started + 1, 'height': 40}, {'at': started + 90, 'height': 40},
                                                   {'at': started + 170, 'height': 40}]}
    return {'clock': 'the runner', 'instants': instants, 'halt_readings': [40, 40],
            'reached': {'validator': 45, 'sentry': 45}, 'compared': {'validator': dict(node), 'sentry': dict(node)},
            'validators_before': [{'validator_id': 'v1', 'power': '100'}],
            'validators_after': [{'validator_id': 'v1', 'power': '100'}], 'host_verify_exit': 0,
            'wal_set_aside': {'host': 'sentry-1', 'moved_to': '/x.f17-1'},
            'signing_guard': {'height': 40}, 'sentry_hold': hold}


def checks(obs, target=TARGET):
    rows, status = rv.evaluate(obs, target)
    return {r['check']: r['status'] for r in rows}, status


class EvaluateTests(unittest.TestCase):
    def test_a_recovery_by_the_procedure_passes(self):
        rows, status = checks(observations())
        self.assertEqual(status, rv.PASS, rows)
        self.assertEqual(set(rows), {'order', 'halt', 'signing_guard', 'sentry_held', 'signing', 'state', 'validator_set',
                                     'host_verify', 'objective'})

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
        for key in ('instants', 'halt_readings', 'reached', 'compared', 'validators_before', 'host_verify_exit',
                    'signing_guard', 'sentry_hold'):
            obs = observations()
            del obs[key]
            rows, status = checks(obs)
            self.assertEqual(status, rv.INCONCLUSIVE, (key, rows))


class GuardAndHoldTests(unittest.TestCase):
    """Incident 2026-11-10: the sentry exited 92 s after its start, the
    replacement found no peer, started a new chain from genesis and signed
    it with the key that had signed the real chain."""

    def test_no_guard_or_a_guard_below_the_halt(self):
        obs = observations()
        del obs['signing_guard']
        self.assertEqual(checks(obs), ({**checks(observations())[0], 'signing_guard': rv.INCONCLUSIVE},
                                       rv.INCONCLUSIVE))
        obs['signing_guard'] = {'height': 39}
        self.assertEqual(checks(obs)[0]['signing_guard'], rv.FAIL)
        obs['signing_guard'] = {'height': 0}
        self.assertEqual(checks(obs)[1], rv.FAIL)

    def test_the_validator_started_before_the_guard_or_the_hold_fails_the_order(self):
        for early in ('guarded_at', 'sentry_held_at'):
            obs = observations()
            obs['instants'][early] = obs['instants']['validator_started_at'] + 1
            self.assertEqual(checks(obs)[0]['order'], rv.FAIL, early)

    def test_the_incident_sentry_exit_fails_the_hold(self):
        obs = observations()
        started = obs['instants']['sentry_started_at']
        obs['sentry_hold']['readings'] = [{'at': started + 15, 'height': 40}, {'at': started + 92, 'height': None}]
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)

    def test_a_hold_under_the_budget_fails(self):
        obs = observations()
        obs['sentry_hold']['readings'] = obs['sentry_hold']['readings'][:2]
        rows, status = checks(obs)
        self.assertEqual((rows['sentry_held'], status), (rv.FAIL, rv.FAIL))
        self.assertEqual(rv.hold_budget(PROCESS), 160)

    def test_a_sentry_past_the_halt_before_the_validator_started_fails(self):
        obs = observations()
        obs['sentry_hold']['readings'][-1]['height'] = 41
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)

    def test_a_hold_reading_after_the_validator_started_or_before_the_sentry_fails(self):
        obs = observations()
        obs['sentry_hold']['readings'].append({'at': obs['instants']['validator_started_at'] + 1, 'height': 40})
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)
        obs = observations()
        obs['sentry_hold']['readings'][0]['at'] = obs['instants']['sentry_started_at'] - 1
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)
        obs = observations()
        obs['sentry_hold']['from'] = obs['instants']['sentry_started_at'] - 1
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)

    def test_a_starting_engine_may_not_answer_within_its_startup_bound_only(self):
        obs = observations()
        started = obs['instants']['sentry_started_at']
        obs['sentry_hold']['readings'].insert(0, {'at': started + 0.5, 'height': None})
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.PASS)
        obs['sentry_hold']['readings'][0]['at'] = started - 1
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)
        obs = observations()
        for r in obs['sentry_hold']['readings'][:2]:
            r['height'] = None
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.FAIL)

    def test_a_hold_without_the_sentry_bounds_is_inconclusive(self):
        obs = observations()
        del obs['sentry_hold']['process']['kill_millis']
        self.assertEqual(checks(obs)[0]['sentry_held'], rv.INCONCLUSIVE)


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

    def step(self, name, guard=40):
        self.clock.sleep(60)
        rv.mark(self.run, name, clock=self.clock, moved_to='/x.f17-1' if name == 'wal_set_aside' else None,
                height=guard if name == 'guarded' else None)

    def hold(self, *heights):
        found = iter(heights or [40] * 20)
        return rv.hold(self.run, lambda: next(found), PROCESS, clock=self.clock, sleep=self.clock.sleep,
                       say=lambda _: None)

    def rest(self, names, halt=40):
        """The remaining console steps, the sentry held before the validator starts."""
        for name in names:
            if name == 'validator_started':
                self.hold(*[halt] * 20)
            self.step(name, guard=halt)

    def until_first_block(self, halt=(40, 40), after=(None, 40, 41)):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        self.assertEqual(rv.halt(self.run, self.pages(*halt), sleep=self.clock.sleep), list(halt))
        self.rest(rv.MARKS[1:], halt=halt[-1])
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
        self.rest(('wal_set_aside', 'guarded', 'sentry_started', 'validator_started'))
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

    def test_start_makes_the_run_directory_before_the_client_writes_into_it(self):
        class WritingClient(Client):
            def __init__(self, directory, *views):
                super().__init__(*views)
                self.directory = directory

            def status(self):
                (self.directory / 'reconcile-status-1.json').write_text('{}')
                return super().status()

        rv.start(self.run, WritingClient(self.run, SET), clock=self.clock)
        self.assertEqual(rv.load_run(self.run)['observations']['validators_before'], SET)

    def test_a_second_client_on_the_run_directory_never_overwrites_the_first_views(self):
        import os, stat, textwrap
        cli = Path(self.directory.name) / 'fake-cli'
        cli.write_text(textwrap.dedent('''\
            #!/usr/bin/env python3
            import json, os, sys
            out = sys.argv[sys.argv.index('--out') + 1]
            fd = os.open(out, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            os.write(fd, json.dumps({'effective_validators': [sys.argv[0][-3:]]}).encode())
            '''))
        cli.chmod(cli.stat().st_mode | stat.S_IXUSR)
        self.run.mkdir(parents=True)
        first = reconcile.Client(cli, dict(os.environ), self.run)
        first.status()
        before = (self.run / 'reconcile-status-1.json').read_bytes()
        second = reconcile.Client(cli, dict(os.environ), self.run)
        second.status()
        second.status()
        self.assertEqual((self.run / 'reconcile-status-1.json').read_bytes(), before)
        self.assertEqual(sorted(p.name for p in self.run.iterdir()),
                         ['reconcile-status-1.json', 'reconcile-status-2.json', 'reconcile-status-3.json'])

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
        self.rest(rv.MARKS[1:])
        self.assertEqual(rv.first_block(self.run, self.pages(41), clock=self.clock, sleep=self.clock.sleep), 41)

    def test_no_halt_height_once_the_votes_are_set_aside(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, None), sleep=self.clock.sleep)
        self.step('wal_set_aside')
        with self.assertRaises(rv.Failed):
            rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        with self.assertRaisesRegex(rv.Failed, 'no halt height'):
            self.rest(rv.MARKS[2:])

    def test_no_first_block_within_the_timeout_fails(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        self.rest(rv.MARKS[1:])
        with self.assertRaises(rv.Failed):
            rv.first_block(self.run, lambda: {'http': 503, 'height': 40}, clock=self.clock,
                           sleep=self.clock.sleep, timeout=30)

    def test_the_reader_gap_is_kept(self):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        self.rest(rv.MARKS[1:])
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

    def until_sentry_started(self, guard=40):
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        for name in ('wal_set_aside', 'installed', 'guarded', 'sentry_started'):
            self.step(name, guard=guard)

    def test_the_hold_reads_past_the_budget_then_says_go(self):
        self.until_sentry_started()
        started = rv.load_run(self.run)['observations']['instants']['sentry_started_at']
        self.assertIn('budget 160 s', self.hold())
        obs = rv.load_run(self.run)['observations']
        self.assertGreaterEqual(obs['instants']['sentry_held_at'] - started, 160)
        self.assertEqual({r['height'] for r in obs['sentry_hold']['readings']}, {40})
        with self.assertRaisesRegex(rv.Failed, 'already held'):
            self.hold()

    def test_the_incident_sentry_exit_stops_the_hold_and_a_new_hold_replaces_it(self):
        self.until_sentry_started()
        with self.assertRaisesRegex(rv.Failed, 'Do not start the validator'):
            self.hold(40, 40, 40, 40, 40, 40, 40, None)
        obs = rv.load_run(self.run)['observations']
        self.assertNotIn('sentry_held_at', obs['instants'])
        self.hold()
        obs = rv.load_run(self.run)['observations']
        self.assertEqual(len(obs['sentry_hold_failed']), 1)
        # The new hold counts from its own start, past the budget again.
        hold = obs['sentry_hold']
        self.assertGreaterEqual(hold['from'], obs['sentry_hold_failed'][0]['readings'][-1]['at'])
        self.assertGreaterEqual(hold['readings'][-1]['at'] - hold['from'], 160)
        self.assertIn('sentry_held_at', obs['instants'])

    def test_the_hold_refuses_another_signer_no_guard_and_a_guard_off_the_halt(self):
        self.until_sentry_started()
        with self.assertRaisesRegex(rv.Failed, 'another signer'):
            self.hold(40, 41)
        self.tearDown()
        self.setUp()
        self.until_sentry_started(guard=39)
        with self.assertRaisesRegex(rv.Failed, 'not H = 40'):
            self.hold()
        self.tearDown()
        self.setUp()
        rv.start(self.run, Client(SET), clock=self.clock)
        self.step('fenced')
        rv.halt(self.run, self.pages(40, 40), sleep=self.clock.sleep)
        self.step('sentry_started')
        with self.assertRaisesRegex(rv.Failed, 'signing guard first'):
            self.hold()
        with self.assertRaisesRegex(rv.Failed, 'takes --height'):
            rv.mark(self.run, 'guarded', clock=self.clock)
        with self.assertRaisesRegex(rv.Failed, 'takes --height'):
            rv.mark(self.run, 'installed', clock=self.clock, height=40)

    def test_a_slow_agent_answer_counts_from_when_the_read_was_sent(self):
        # Each read takes 100 s to come back: the reading sent at 200 s is
        # the first past the 160 s budget, not the one received at 200 s.
        self.until_sentry_started()
        start = self.clock.now

        def slow():
            self.clock.sleep(100)
            return 40
        rv.hold(self.run, slow, PROCESS, clock=self.clock, sleep=self.clock.sleep, period=0, say=lambda _: None)
        readings = rv.load_run(self.run)['observations']['sentry_hold']['readings']
        self.assertEqual([r['at'] - start for r in readings], [0, 100, 200])

    def test_the_hold_is_before_the_validator_starts(self):
        self.until_sentry_started()
        self.step('validator_started')
        with self.assertRaisesRegex(rv.Failed, 'already started'):
            self.hold()

    def test_a_sentry_below_the_halt_after_the_start_says_stop_the_validator(self):
        self.until_sentry_started()
        self.hold()
        self.step('validator_started')
        with self.assertRaisesRegex(rv.Failed, 'stop the validator at once'):
            rv.first_block(self.run, self.pages(40, 3), clock=self.clock, sleep=self.clock.sleep)

    def test_the_agent_height_is_the_highest_status_read(self):
        self.assertEqual(rv.agent_height(AgentChannel('sentry-1', [39, 40])), 40)
        self.assertIsNone(rv.agent_height(AgentChannel('sentry-1', [40], status='EXPIRED')))

    def test_the_commands_are_wired(self):
        import subprocess
        import sys
        out = subprocess.run([sys.executable, rv.__file__, '--help'], capture_output=True, text=True).stdout
        for command in ('verify', 'start', 'mark', 'halt', 'hold', 'first-block', 'finish'):
            self.assertIn(command, out)


class StagingTests(unittest.TestCase):
    def test_the_drill_is_wired(self):
        import staging_network
        self.assertTrue(callable(staging_network.validator_recovery_drill))
        self.assertIs(staging_network.OPERATOR_STATUS, __import__('reconcile').OPERATOR_STATUS)

    def test_the_drill_guards_and_holds_before_the_validator_starts(self):
        import inspect
        import staging_network
        drill = inspect.getsource(staging_network.validator_recovery_drill)
        guard, hold, start = (drill.index(s) for s in ("'guard', str(halt)", 'recovery.held(', "'dytallix-node.service'"))
        self.assertLess(guard, hold)
        self.assertLess(hold, start)
        self.assertNotIn("'guard'", inspect.getsource(staging_network.restore_drill))


if __name__ == '__main__':
    unittest.main()
