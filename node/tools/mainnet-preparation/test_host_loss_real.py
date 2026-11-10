"""Host loss on the real staging sentry (T5; P01, 10 November 2026,
launch/approvals/P01_T05_HOST_LOSS_REAL_2026-11-10.json), with a fake hcloud
and a fake status page only: no provider call and no host is touched. Covers
the server guard, the dry run, success, the recovery timeout, and that the
power-on always follows a power-off that was sent."""
from datetime import datetime, timezone
import json
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import faults  # noqa: E402
import load_machines  # noqa: E402

SENTRY = {'id': 120260174, 'name': 'ubuntu-4gb-hel1-2', 'status': 'running',
          'public_net': {'ipv4': {'ip': '89.167.52.129'}},
          'location': {'name': 'hel1'}, 'datacenter': {'name': 'hel1-dc2', 'location': {'name': 'hel1'}}}


def fast(_seconds):
    time.sleep(0.01)


class FakeHcloud:
    """hcloud's subprocess.run: one server, its power state, and every call."""

    def __init__(self, server=None, poweroff_fails=False, stays_on=False, poweron_fails=0, boot_seconds=0.2):
        self.server = dict(SENTRY if server is None else server)
        self.poweroff_fails, self.stays_on, self.poweron_fails = poweroff_fails, stays_on, poweron_fails
        self.boot_seconds, self.calls, self.booted_at = boot_seconds, [], None
        self.envs = []

    def __call__(self, command, capture_output=True, text=True, env=None, timeout=None):
        self.calls.append(command)
        self.envs.append(env)
        assert command[:3] == ['hcloud', '--context', 'dytallix'], command
        verb = command[3:5]

        def done(code=0, out=''):
            return subprocess.CompletedProcess(command, code, out, '' if code == 0 else 'boom')
        if verb == ['server', 'describe']:
            return done(out=json.dumps(self.server))
        if verb == ['server', 'poweroff']:
            if not self.stays_on:
                self.server['status'] = 'off'
            return done(1 if self.poweroff_fails else 0)
        if verb == ['server', 'poweron']:
            if self.poweron_fails:
                self.poweron_fails -= 1
                return done(1)
            self.server['status'], self.booted_at = 'running', time.time()
            return done()
        return done(1)

    def verbs(self):
        return [c[4] for c in self.calls]

    def power_calls(self):
        return [v for v in self.verbs() if v in ('poweroff', 'poweron')]


class FakePage:
    """The sentry's status page: refused while off or booting; after that its
    height follows the clock (unless it never comes back)."""

    def __init__(self, cloud, comes_back=True):
        self.cloud, self.comes_back, self.height, self.last = cloud, comes_back, 100, time.time()

    def read(self):
        now = time.time()
        up = self.cloud.server['status'] == 'running' and (
            self.cloud.booted_at is None or (self.comes_back and now >= self.cloud.booted_at + self.cloud.boot_seconds))
        if not up:
            return {'http': None, 'refused': True, 'read_at': faults.iso_at(now)}
        if now - self.last >= 0.05:
            self.height, self.last = self.height + 1, now
        made = datetime.fromtimestamp(self.last, tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')
        return {'http': 200, 'chain_id': 'c', 'height': self.height, 'time': made, 'read_at': faults.iso_at(now)}


def made(page):
    text = page.get('time')
    return datetime.fromisoformat(text.replace('Z', '+00:00')).timestamp() if text else None


SPEC = dict(faults.HOST_LOSS, args={'off_seconds': 0.3})


class Base(unittest.TestCase):
    def power(self, cloud, **over):
        hcloud = load_machines.Hcloud('dytallix', 'hcloud', run=cloud)
        return faults.HcloudPower(hcloud, sleep=fast, say=lambda m: None, **over)

    def drive(self, cloud, page=None, timeout=5, **over):
        power = self.power(cloud, **over)
        page = page or FakePage(cloud)
        row = faults.run_real_host_loss(power, page.read, made, SPEC, recovery_timeout=timeout, sleep=fast,
                                        say=lambda m: None, period=0.02)
        return power, faults.build_run('c', b'{}', b'{}', [row], 600, faults.SENTRY_HOST, power=power.record())


class Guard(Base):
    def refused(self, server):
        cloud = FakeHcloud(server)
        with self.assertRaises(faults.Failed) as caught:
            self.drive(cloud)
        self.assertEqual(cloud.power_calls(), [], 'a refused server is never powered off or on')
        return str(caught.exception)

    def test_the_validator_is_refused(self):
        self.assertIn('never power-cycled', self.refused(
            dict(SENTRY, id=131186483, name='ubuntu-4gb-fsn1-1', location={'name': 'fsn1'},
                 datacenter={'location': {'name': 'fsn1'}})))

    def test_the_web_server_is_refused(self):
        self.assertIn('never power-cycled', self.refused(
            dict(SENTRY, id=99, name='docker-ce-ubuntu-4gb-ash-1', location={'name': 'ash'}, datacenter={})))

    def test_the_sentry_s_name_on_another_id_is_refused(self):
        self.assertIn('not the staging sentry', self.refused(dict(SENTRY, id=120260175)))

    def test_the_sentry_s_id_with_another_name_is_refused(self):
        self.assertIn('not the staging sentry', self.refused(dict(SENTRY, name='ubuntu-4gb-hel1-3')))

    def test_another_location_is_refused(self):
        self.assertIn('location', self.refused(dict(SENTRY, location={'name': 'fsn1'})))
        self.assertIn('location', self.refused(dict(SENTRY, datacenter={'location': {'name': 'nbg1'}})))

    def test_no_location_is_refused(self):
        self.assertIn('none', self.refused(dict(SENTRY, location=None, datacenter={})))

    def test_the_real_describe_output_passes_the_guard(self):
        # Trimmed from `hcloud --context dytallix server describe 120260174 -o
        # json` (10 November 2026): the location is top-level only, with no
        # datacenter field.
        real = json.loads((Path(__file__).resolve().parent / 'fixtures' / 'hcloud-sentry-describe.json').read_bytes())
        self.assertIs(faults.check_sentry(real), real)
        self.assertNotIn('datacenter', real)
        cloud = FakeHcloud(real)
        _, record = self.drive(cloud)
        self.assertEqual(record['scenarios'][0]['status'], 'PASS')
        self.assertIn('not the staging sentry', self.refused(dict(real, id=131186484)))

    def test_another_address_is_refused(self):
        self.assertIn('IPv4', self.refused(dict(SENTRY, public_net={'ipv4': {'ip': '89.167.52.130'}})))
        self.assertIn('IPv4', self.refused(dict(SENTRY, public_net={})))

    def test_every_call_names_the_context_and_drops_hcloud_variables(self):
        cloud = FakeHcloud()
        with mock.patch.dict('os.environ', {'HCLOUD_TOKEN': 'x', 'HCLOUD_CONTEXT': 'other'}):
            self.drive(cloud)
        self.assertTrue(cloud.calls)
        for env in cloud.envs:
            self.assertFalse([k for k in env if k.startswith('HCLOUD_')])
        for command in cloud.calls:
            self.assertEqual(command[5], '120260174', 'every call addresses the sentry by its id')


class Drive(Base):
    def test_success_is_a_real_host_pass(self):
        cloud = FakeHcloud()
        power, record = self.drive(cloud)
        built = record['scenarios'][0]
        self.assertEqual((built['kind'], built['status']), ('host', 'PASS'), built)
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron'])
        self.assertEqual(cloud.verbs()[:3], ['describe', 'describe', 'poweroff'], 'guarded twice before the power-off')
        self.assertEqual(record['host'], 'sentry-1')
        self.assertEqual(record['power'], {'controller': 'hcloud', 'real_host': True, 'context': 'dytallix',
                                           'server': {'id': 120260174, 'name': 'ubuntu-4gb-hel1-2',
                                                      'location': 'hel1'}})
        self.assertEqual(faults.verify(json.loads(json.dumps(record)), b'{}', b'{}'), 'PASS')
        edited = json.loads(json.dumps(record))
        edited['power']['real_host'] = False
        with self.assertRaises(faults.Failed):
            faults.verify(edited, b'{}', b'{}')

    def test_a_sentry_that_does_not_come_back_in_time_fails(self):
        cloud = FakeHcloud()
        _, record = self.drive(cloud, FakePage(cloud, comes_back=False), timeout=0.5)
        self.assertEqual(record['scenarios'][0]['status'], 'FAIL', record['scenarios'][0])
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron'])

    def test_a_recovery_over_the_target_fails(self):
        cloud = FakeHcloud()
        power, record = self.drive(cloud)
        row = dict(record['scenarios'][0])
        cleared = faults.instant(row['cleared_at']).timestamp()
        row['recovered_at'] = faults.iso_at(cleared + 601)
        self.assertEqual(faults.evaluate(row, 600)[0], 'FAIL')


class AlwaysOn(Base):
    def test_a_failed_power_off_is_still_followed_by_a_power_on(self):
        cloud = FakeHcloud(poweroff_fails=True)
        _, record = self.drive(cloud, timeout=0.5)
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron'])
        self.assertEqual(record['scenarios'][0]['status'], 'FAIL')

    def test_a_machine_never_reported_off_is_still_powered_on(self):
        cloud = FakeHcloud(stays_on=True)
        _, record = self.drive(cloud, timeout=0.5, confirm_seconds=0.2)
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron'])
        self.assertEqual(record['scenarios'][0]['status'], 'FAIL')

    def test_a_failing_power_on_is_retried(self):
        cloud = FakeHcloud(poweron_fails=2)
        _, record = self.drive(cloud)
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron', 'poweron', 'poweron'])
        self.assertEqual(record['scenarios'][0]['status'], 'PASS')
        cloud = FakeHcloud(poweron_fails=5)
        _, record = self.drive(cloud, timeout=0.3)
        self.assertEqual(cloud.power_calls(), ['poweroff'] + ['poweron'] * faults.POWER_ON_TRIES)
        self.assertEqual(record['scenarios'][0]['status'], 'FAIL')

    def test_an_error_during_the_hold_still_powers_on(self):
        cloud = FakeHcloud()
        page = FakePage(cloud)
        calls = {'n': 0}

        def breaks_while_off(_seconds):
            if cloud.server['status'] == 'off' and threading.current_thread() is threading.main_thread():
                calls['n'] += 1
                if calls['n'] == 3:
                    raise RuntimeError('the founder\'s machine failed')
            time.sleep(0.01)
        power = self.power(cloud)
        with self.assertRaises(RuntimeError):
            faults.run_real_host_loss(power, page.read, made, SPEC, recovery_timeout=1, sleep=breaks_while_off,
                                      say=lambda m: None, period=0.02)
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron'])
        self.assertEqual(cloud.server['status'], 'running')

    def test_an_interrupt_during_the_hold_still_powers_on(self):
        cloud = FakeHcloud()
        power = self.power(cloud)
        spec = dict(faults.HOST_LOSS, args={'off_seconds': 30})

        def interrupt():
            while cloud.server['status'] != 'off':
                time.sleep(0.01)
            signal.raise_signal(signal.SIGTERM)
        before = signal.getsignal(signal.SIGTERM)
        threading.Thread(target=interrupt, daemon=True).start()
        with self.assertRaises(faults.Failed) as caught:
            faults.run_real_host_loss(power, FakePage(cloud).read, made, spec, recovery_timeout=1, sleep=fast,
                                      say=lambda m: None, period=0.02)
        self.assertIn('signal', str(caught.exception))
        self.assertEqual(cloud.power_calls(), ['poweroff', 'poweron'])
        self.assertIs(signal.getsignal(signal.SIGTERM), before, 'the handlers are restored')


class Command(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        (self.dir / 'native.json').write_bytes(b'{}')
        (self.dir / 'engine.json').write_bytes(b'{}')
        self.lines = []

    def args(self, execute):
        return SimpleNamespace(chain='dytallix-staging-2', genesis=self.dir / 'native.json',
                               engine_genesis=self.dir / 'engine.json', out=self.dir / 'evidence.json',
                               context='dytallix', hcloud='hcloud', execute=execute)

    def test_the_dry_run_prints_the_calls_and_runs_nothing(self):
        def never(*a, **k):
            raise AssertionError('the dry run ran hcloud')
        code = faults.host_loss_main(self.args(False), run=never, read_page=never, say=self.lines.append)
        self.assertEqual(code, 0)
        text = '\n'.join(self.lines)
        for call in ('hcloud --context dytallix server describe 120260174 -o json',
                     'hcloud --context dytallix server poweroff 120260174',
                     'hcloud --context dytallix server poweron 120260174'):
            self.assertIn(call, text)
        self.assertIn(faults.SENTRY_STATUS_URL, text)
        self.assertIn('600 s', text)
        self.assertFalse((self.dir / 'evidence.json').exists())

    def test_execute_writes_a_verified_real_host_record_once(self):
        cloud = FakeHcloud()
        code = faults.host_loss_main(self.args(True), run=cloud, read_page=FakePage(cloud).read, sleep=fast,
                                     say=self.lines.append, spec=SPEC, period=0.02, recovery_timeout=5)
        self.assertEqual(code, 0, self.lines)
        record = json.loads((self.dir / 'evidence.json').read_bytes())
        self.assertEqual((record['status'], record['recovery_target_seconds'], record['chain_id']),
                         ('PASS', 600, 'dytallix-staging-2'))
        self.assertTrue(record['power']['real_host'])
        again = FakeHcloud()
        self.assertEqual(faults.host_loss_main(self.args(True), run=again, say=self.lines.append), 1)
        self.assertEqual(again.calls, [], 'a second run is refused before any call')


if __name__ == '__main__':
    unittest.main()
