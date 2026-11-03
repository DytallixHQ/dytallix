"""The load machines (P01_T06_LOAD_MACHINES_HETZNER_2026-11-03): the limits
are in code, every machine is destroyed whatever happens, nothing overlaps,
and nothing outside the load label is ever deleted. The hcloud CLI is a
stand-in here that keeps a project in a file; the tool runs it as a real
program."""
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import sys
import tempfile
import textwrap
import unittest

import load_machines as lm

CONTEXT = 'dytallix-test'

# The few hcloud commands the tool uses, with hcloud's JSON shapes. A
# protected server refuses deletion, as Hetzner's API does.
FAKE = textwrap.dedent('''\
    import json, os, sys
    path = os.environ['DYTALLIX_FAKE_HCLOUD']
    project = json.load(open(path))
    argv = sys.argv[1:]
    project['calls'].append({'argv': argv, 'token_in_env': any(k.startswith('HCLOUD_') for k in os.environ)})

    def done(value=None, code=0, error=''):
        json.dump(project, open(path, 'w'))
        if value is not None:
            print(json.dumps(value))
        if error:
            print('hcloud: ' + error, file=sys.stderr)
        sys.exit(code)

    if argv[:2] != ['--context', project['context']]:
        done(code=1, error='context not found')
    argv = argv[2:]
    if argv[-2:] == ['-o', 'json']:
        argv = argv[:-2]

    def flag(name, many=False):
        found = [argv[i + 1] for i, a in enumerate(argv) if a == name]
        return found if many else (found[0] if found else None)

    def match(item, selector):
        key, _, value = selector.partition('=')
        return project['ignore_selector'] or key in item['labels'] and (not value or item['labels'][key] == value)

    def label():
        key, _, value = flag('--label').partition('=')
        return {key: value}
    what, verb = argv[0], argv[1]
    if (what, verb) == ('server', 'list'):
        found = [s for s in project['servers'] if flag('-l') is None or match(s, flag('-l'))]
        for s in found:
            if s['status'] == 'initializing' and not project['never_running']:
                project['next_ip'] += 1
                s['status'], s['public_net']['ipv4'] = 'running', {'ip': '203.0.113.%d' % project['next_ip']}
        done(found)
    if (what, verb) == ('server', 'create'):
        project['next_id'] += 1
        server = {'id': project['next_id'], 'name': flag('--name'), 'status': 'initializing', 'labels': label(),
                  'location': {'name': flag('--location')}, 'server_type': {'name': flag('--type')},
                  'image': flag('--image'), 'ssh_keys': flag('--ssh-key', many=True),
                  'ipv6': '--without-ipv6' not in argv, 'protection': {'delete': False, 'rebuild': False},
                  'public_net': {'ipv4': None}}
        project['servers'].append(server)
        done({'server': server, 'root_password': None})
    if (what, verb) == ('server', 'delete'):
        for server_id in argv[2:]:
            server = next((s for s in project['servers'] if str(s['id']) == server_id), None)
            if server is None:
                done(code=1, error='server not found')
            if server['protection']['delete']:
                done(code=1, error='server deletion is protected')
            project['servers'].remove(server)
        done()
    if (what, verb) == ('server-type', 'describe'):
        done({'name': argv[2], 'locations': [{'name': n, 'available': n not in project['sold_out'], 'deprecation': None}
                                             for n in ('fsn1', 'nbg1', 'hel1', 'sin')]})
    if (what, verb) == ('ssh-key', 'create'):
        project['next_id'] += 1
        key = {'id': project['next_id'], 'name': flag('--name'), 'labels': label(),
               'public_key': open(flag('--public-key-from-file')).read().strip()}
        project['keys'].append(key)
        done({'ssh_key': key})
    if (what, verb) == ('ssh-key', 'list'):
        done([k for k in project['keys'] if match(k, flag('-l'))])
    if (what, verb) == ('ssh-key', 'delete'):
        project['keys'] = [k for k in project['keys'] if str(k['id']) != argv[2]]
        done()
    done(code=1, error='unknown command ' + ' '.join(argv))
''')


def host(server_id, name, protected=True):
    return {'id': server_id, 'name': name, 'status': 'running', 'labels': {}, 'location': {'name': 'hel1'},
            'protection': {'delete': protected, 'rebuild': protected}, 'public_net': {'ipv4': {'ip': '198.51.100.1'}}}


class LoadMachinesTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.project = root / 'project.json'
        self.save({'context': CONTEXT, 'next_id': 100, 'next_ip': 0, 'never_running': False, 'sold_out': [],
                   'ignore_selector': False,
                   'servers': [host(1, 'ubuntu-4gb-hel1-2'), host(2, 'ubuntu-4gb-fsn1-1')],
                   'keys': [{'id': 7, 'name': 'founder@example.com', 'labels': {}}], 'calls': []})
        fake = root / 'hcloud'
        fake.write_text(f'#!{sys.executable}\n' + FAKE)
        fake.chmod(0o755)
        os.environ['DYTALLIX_FAKE_HCLOUD'] = str(self.project)
        self.addCleanup(os.environ.pop, 'DYTALLIX_FAKE_HCLOUD', None)
        self.cloud = lm.Hcloud(CONTEXT, fake)
        self.out = root / 'run'
        self.lines = []

    def save(self, project):
        self.project.write_text(json.dumps(project))

    def state(self):
        return json.loads(self.project.read_text())

    def change(self, **values):
        project = self.state()
        project.update(values)
        self.save(project)

    def up(self, count=4, **changes):
        return lm.up(self.cloud, self.out, count, changes.pop('hours', 2), sleep=lambda s: None,
                     say=self.lines.append, **changes)

    def names(self):
        return sorted(s['name'] for s in self.state()['servers'])

    def test_up_spreads_the_machines_labels_them_and_down_destroys_them(self):
        watched = []
        state = self.up(4, watchdog=watched.append)
        self.assertEqual([m['location'] for m in state['machines']], ['nbg1', 'fsn1', 'sin', 'nbg1'])
        self.assertEqual(len({m['ipv4'] for m in state['machines']}), 4)
        made = [s for s in self.state()['servers'] if s['name'].startswith(lm.TAG)]
        self.assertTrue(all(s['labels'] == {lm.TAG: state['run']} and s['image'] == lm.IMAGE and not s['ipv6']
                            and s['ssh_keys'] == [str(state['key_id'])] and s['server_type']['name'] == 'cpx02'
                            for s in made))
        self.assertEqual(watched, [self.out], 'the deadline watchdog starts once the key exists')
        self.assertEqual(oct(self.out.stat().st_mode & 0o777), '0o700')
        self.assertEqual(json.loads((self.out / lm.STATE).read_text())['machines'], state['machines'])
        lm.down(self.cloud, self.out, say=self.lines.append)
        self.assertEqual(self.names(), ['ubuntu-4gb-fsn1-1', 'ubuntu-4gb-hel1-2'])
        self.assertEqual([k['name'] for k in self.state()['keys']], ['founder@example.com'])

    def test_the_context_is_named_on_every_call_and_no_environment_token_reaches_hcloud(self):
        os.environ['HCLOUD_TOKEN'] = 'x' * 64
        self.addCleanup(os.environ.pop, 'HCLOUD_TOKEN', None)
        lm.up(lm.Hcloud(CONTEXT, self.cloud.binary), self.out, 1, 1, sleep=lambda s: None, say=self.lines.append)
        calls = self.state()['calls']
        self.assertTrue(calls and all(c['argv'][:2] == ['--context', CONTEXT] and not c['token_in_env'] for c in calls))
        with self.assertRaisesRegex(lm.Failed, 'context not found'):
            lm.Hcloud('another', self.cloud.binary).servers(lm.TAG)
        with self.assertRaisesRegex(lm.Failed, 'not an hcloud context name'):
            lm.Hcloud('a b', self.cloud.binary)

    def test_the_limits_are_in_code(self):
        for count, hours, kind, places, why in ((9, 2, 'cpx02', lm.LOCATIONS, 'machines'),
                                                (0, 2, 'cpx02', lm.LOCATIONS, 'machines'),
                                                (4, 13, 'cpx02', lm.LOCATIONS, 'hours'),
                                                (4, 2, 'cx33', lm.LOCATIONS, 'approved smallest'),
                                                (4, 2, 'cpx02', ('nbg1', 'hel1'), 'approved location')):
            with self.subTest(why=why), self.assertRaisesRegex(lm.Failed, why):
                self.up(count, hours=hours, server_type=kind, locations=places)
        self.assertEqual(self.names(), ['ubuntu-4gb-fsn1-1', 'ubuntu-4gb-hel1-2'], 'a refused plan rents nothing')

    def test_up_refuses_while_a_host_is_unprotected_or_the_type_is_sold_out(self):
        project = self.state()
        project['servers'][1]['protection'] = {'delete': False, 'rebuild': False}
        self.save(project)
        with self.assertRaisesRegex(lm.Failed, 'without delete protection share the project \\(ubuntu-4gb-fsn1-1\\)'):
            self.up(2)
        project['servers'][1]['protection'] = {'delete': True, 'rebuild': True}
        project['sold_out'] = ['sin']
        self.save(project)
        with self.assertRaisesRegex(lm.Failed, 'cannot be ordered in sin'):
            self.up(3)
        self.assertEqual(len(self.state()['servers']), 2)
        self.assertEqual(len(self.state()['keys']), 1, 'nothing registered before the checks pass')

    def test_a_second_run_never_overlaps_a_machine_left_running(self):
        self.up(2)
        with self.assertRaisesRegex(lm.Failed, 'still exist'):
            lm.up(self.cloud, Path(self.tmp.name) / 'second', 2, 2, sleep=lambda s: None, say=self.lines.append)
        self.assertEqual(len(self.state()['servers']), 4)

    def test_a_failed_run_destroys_what_it_made(self):
        self.change(never_running=True)
        ticks = iter(range(0, 10_000, 100))
        with self.assertRaisesRegex(lm.Failed, 'only 0 of 3 machines running'):
            self.up(3, clock=lambda: next(ticks), timeout=500)
        self.assertEqual(self.names(), ['ubuntu-4gb-fsn1-1', 'ubuntu-4gb-hel1-2'])
        self.assertEqual([k['name'] for k in self.state()['keys']], ['founder@example.com'])

    def test_reap_removes_only_the_tools_machines_and_keys(self):
        self.up(3)
        project = self.state()
        # Unlabelled but named like a machine, labelled but protected, and
        # labelled but named otherwise: all stay.
        project['servers'] += [host(50, 'dytallix-load-lookalike', protected=False),
                               dict(host(51, 'dytallix-load-kept'), labels={lm.TAG: 'old'}),
                               dict(host(53, 'someone-else', protected=False), labels={lm.TAG: 'old'})]
        project['keys'] += [{'id': 52, 'name': 'dytallix-load-unlabelled', 'labels': {}},
                            {'id': 54, 'name': 'someone-elses-key', 'labels': {lm.TAG: 'old'}}]
        self.save(project)
        self.assertEqual(lm.reap(self.cloud, say=self.lines.append), (3, 1))
        kept = ['dytallix-load-kept', 'dytallix-load-lookalike', 'someone-else', 'ubuntu-4gb-fsn1-1', 'ubuntu-4gb-hel1-2']
        self.assertEqual(self.names(), kept)
        keys = ['dytallix-load-unlabelled', 'founder@example.com', 'someone-elses-key']
        self.assertEqual(sorted(k['name'] for k in self.state()['keys']), keys)
        # Even a listing that ignored its selector would delete nothing else.
        self.change(ignore_selector=True)
        self.assertEqual(lm.reap(self.cloud, say=self.lines.append), (0, 0))
        self.assertEqual((self.names(), sorted(k['name'] for k in self.state()['keys'])), (kept, keys))

    def test_down_deletes_only_the_recorded_machines(self):
        self.up(2)
        record = json.loads((self.out / lm.STATE).read_text())
        project = self.state()
        project['servers'].append(dict(host(60, f'{lm.TAG}-{record["run"]}-9', protected=False),
                                       labels={lm.TAG: record['run']}))
        self.save(project)
        lm.down(self.cloud, self.out, say=self.lines.append)
        self.assertEqual(self.names(), [f'{lm.TAG}-{record["run"]}-9', 'ubuntu-4gb-fsn1-1', 'ubuntu-4gb-hel1-2'])

    def test_a_listing_never_runs_without_a_selector(self):
        for empty in ('', None):
            with self.subTest(selector=empty), self.assertRaisesRegex(lm.Failed, 'needs a label selector'):
                self.cloud.servers(empty)

    def test_the_watchdog_destroys_the_run_at_its_deadline(self):
        state = self.up(2)
        deadline = datetime.strptime(state['deadline'], '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
        times = iter([deadline - timedelta(minutes=5), deadline - timedelta(seconds=1), deadline])
        slept = []
        lm.watch(self.cloud, self.out, clock=lambda: next(times), sleep=slept.append, say=self.lines.append)
        self.assertEqual(self.names(), ['ubuntu-4gb-fsn1-1', 'ubuntu-4gb-hel1-2'])
        self.assertEqual(slept, [60, 1])

    def test_the_watchdog_stops_once_down_has_destroyed_the_run(self):
        self.up(1)
        lm.down(self.cloud, self.out, say=self.lines.append)
        slept = []
        lm.watch(self.cloud, self.out, clock=lambda: datetime.now(timezone.utc), sleep=slept.append, say=self.lines.append)
        self.assertEqual((slept, self.lines[-1]), ([], '== the run was destroyed by down; the watchdog stops'))


if __name__ == '__main__':
    unittest.main()
