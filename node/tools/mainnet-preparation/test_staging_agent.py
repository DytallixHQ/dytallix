"""The staging agent (simulation plan, T2): what it runs, what it refuses,
that a slot is spent before a command runs, and how results leave the host.
curl, the signer and the CLI are stand-ins here; the ML-DSA-65 signatures
are the release CLI's own, and the CI staging network drives the real ones
(staging_network.py agent-drill)."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'staging-agent'))
sys.path.insert(0, str(HERE))
import agent  # noqa: E402
import staging_command  # noqa: E402

CHAIN, LABEL, RELEASE = 'dytallix-staging-1', 'sentry-1', '0123456789abcdef'
BIN = f'/opt/dytallix/{RELEASE}/bin'
KEY = '11' * 1952
OTHER_KEY = '22' * 1952
STORE = {'endpoint': 'https://store.example', 'bucket': 'copies', 'region': 'auto', 'prefix': 'staging-commands/',
         'access_key_id': 'READKEY', 'secret_access_key': 'cmVhZA=='}
HISTORY = dict(STORE, schema=agent.UPLOAD_SCHEMA, prefix='staging-history/', access_key_id='HISTORYKEY')
ALLOWLIST = agent.check_allowlist(json.loads((HERE / 'staging-agent' / 'ALLOWLIST.json').read_bytes()))


def fake_signature(key, text):
    """The stand-in scheme: deterministic in the key and the message."""
    return (hashlib.sha256((key + text).encode()).hexdigest() * 104)[:agent.SIGNATURE_HEX]


def fake_verify(text, signature, key):
    return signature == fake_signature(key, text)


def signed(sequence, command='status', args=None, chain=CHAIN, host=LABEL, not_after=1_900_003_600, key=KEY):
    body = staging_command.body(chain, host, sequence, command, args or {}, not_after)
    return json.dumps({'body': body, 'signature': fake_signature(key, agent.message(body))}).encode()


def settings():
    return {'schema': agent.SETTINGS_SCHEMA, 'label': LABEL, 'chain_id': CHAIN, 'command_public_key_hex': KEY,
            'commands': STORE}


class Store:
    """The command store and the result store, behind a stand-in curl."""

    def __init__(self):
        self.objects, self.uploads, self.status, self.fail_uploads = {}, {}, None, False

    def curl(self, config):
        lines = dict(line.split(' = ', 1) for line in config.decode().splitlines() if ' = ' in line)
        url = lines['url'].strip('"')
        if 'upload-file' in lines:
            if self.fail_uploads:
                return subprocess.CompletedProcess([], 22, b'', b'upload refused')
            assert 'HISTORYKEY' in lines['user']
            self.uploads[url] = Path(lines['upload-file'].strip('"')).read_bytes()
            return subprocess.CompletedProcess([], 0, b'', b'')
        assert 'READKEY' in lines['user'] and lines['max-filesize'] == str(agent.MAX_COMMAND_BYTES)
        if self.status:
            return subprocess.CompletedProcess([], 0, str(self.status).encode(), b'')
        raw = self.objects.get(url)
        if raw is not None and len(raw) > agent.MAX_COMMAND_BYTES:
            return subprocess.CompletedProcess([], 63, b'', b'maximum file size exceeded')
        Path(lines['output'].strip('"')).write_bytes(raw if raw is not None else b'no such key')
        return subprocess.CompletedProcess([], 0, b'200' if raw is not None else b'404', b'')

    def put(self, sequence, raw):
        self.objects[f'{STORE["endpoint"]}/{STORE["bucket"]}/{STORE["prefix"]}{CHAIN}/{LABEL}/{sequence:010}.json'] = raw


class FakeHost(agent.Host):
    def __init__(self, root, store):
        super().__init__(root, self.execute)
        self.store, self.commands = store, []

    def execute(self, argv, capture_output=True, input=None, env=None, timeout=None, check=False):
        name = Path(argv[0]).name
        if name == 'curl':
            return self.store.curl(input)
        if name == 'dytallix-root-sign':
            flags = dict(zip(argv[2::2], argv[3::2]))
            assert argv[1] == 'history-seal' and flags['-host'] == LABEL
            Path(flags['-out']).write_bytes(b'SEALED:' + Path(flags['-in']).read_bytes())
            return subprocess.CompletedProcess(argv, 0, b'', b'')
        if name == 'dytallix':
            assert argv[1:3] == ['crypto', 'verify'] and env['HOME'].endswith('/var/lib/dytallix-agent/cli-home')
            return subprocess.CompletedProcess(argv, 0 if fake_verify(*argv[3:6]) else 1, b'', b'')
        self.commands.append(argv)
        return subprocess.CompletedProcess(argv, 0, b'active\n', b'')


class AgentTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name) / 'host'
        self.store = Store()
        self.host = FakeHost(self.root, self.store)
        self.now = 1_900_000_000
        for base in ('/opt', '/var/lib', '/etc/systemd/system'):
            self.host.path(base).mkdir(parents=True, exist_ok=True)
        node_unit = self.host.path(agent.NODE_UNIT_FILE)
        node_unit.parent.mkdir(parents=True, exist_ok=True)
        node_unit.write_text(f'[Service]\nExecStart={BIN}/dytallix-native-supervisor --service-config x\n')
        monitor = {'schema': 'dytallix.host-monitor.v1', 'label': LABEL, 'chain_id': CHAIN, 'curl': '/usr/bin/curl',
                   'history_upload': {'signer': f'{BIN}/dytallix-root-sign', 'code_file': '/etc/dytallix-monitor/history-code',
                                      'upload_file': '/etc/dytallix-monitor/history-upload.json'}}
        self.write(f'/etc/dytallix/{RELEASE}/monitor.json', json.dumps(monitor))
        self.write('/etc/dytallix-monitor/history-upload.json', json.dumps(HISTORY))
        self.write('/etc/dytallix-monitor/history-code', 'dytallix-history-...\n')
        self.write(f'{BIN}/dytallix', 'the CLI')
        self.settings = Path(tmp.name) / 'settings.json'
        self.settings.write_text(json.dumps(settings()))
        agent.install(self.host, self.settings, out=lambda _: None)

    def write(self, absolute, text):
        path = self.host.path(absolute)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def poll(self):
        return agent.poll(self.host, clock=lambda: self.now, out=lambda _: None)

    def results(self):
        found = {}
        for url, raw in self.store.uploads.items():
            self.assertTrue(raw.startswith(b'SEALED:'))
            self.assertIn(f'/staging-history/{CHAIN}/agent/{LABEL}/', url)
            result = json.loads(raw[len(b'SEALED:'):])
            found[result['sequence']] = result
        return found

    def test_install_places_the_agent_root_only_and_refuses_twice(self):
        self.assertEqual(stat.S_IMODE(os.stat(self.host.path(agent.SETTINGS)).st_mode), 0o400)
        self.assertEqual(stat.S_IMODE(os.stat(self.host.path(agent.ETC)).st_mode), 0o700)
        self.assertEqual(stat.S_IMODE(os.stat(self.host.path(f'{agent.OPT}/agent.py')).st_mode), 0o555)
        self.assertIn('ExecStart=/usr/bin/python3 -I -B /opt/dytallix-agent/agent.py poll',
                      self.host.path(agent.UNIT_FILE).read_text())
        self.assertIn(['systemctl', 'enable', '--now', 'dytallix-agent.timer'], self.host.commands)
        with self.assertRaisesRegex(agent.Failed, 'already installed'):
            agent.install(self.host, self.settings, out=lambda _: None)
        agent.remove(self.host, out=lambda _: None)
        self.assertEqual(agent.installed(self.host), [])

    def test_install_refuses_another_host_or_no_node(self):
        agent.remove(self.host, out=lambda _: None)
        other = self.settings.with_name('other.json')
        other.write_text(json.dumps(dict(settings(), label='validator-1')))
        with self.assertRaisesRegex(agent.Failed, 'this host is sentry-1'):
            agent.install(self.host, other, out=lambda _: None)
        os.unlink(self.host.path(agent.NODE_UNIT_FILE))
        with self.assertRaisesRegex(agent.Failed, 'install the node first'):
            agent.install(self.host, self.settings, out=lambda _: None)
        self.assertEqual(agent.installed(self.host), [])

    def test_runs_signed_commands_and_refuses_the_rest(self):
        self.store.put(1, signed(1, 'status'))
        tampered = json.loads(signed(2, 'status'))
        tampered['body']['command'] = 'stop'
        self.store.put(2, json.dumps(tampered).encode())
        self.store.put(3, signed(1, 'status'))                                   # a replay of 1
        self.store.put(4, signed(4, 'shell'))                                    # not allowlisted
        self.store.put(5, json.dumps({'body': json.loads(signed(5))['body']}).encode())  # unsigned
        self.store.put(6, signed(6, key=OTHER_KEY))                              # another key
        self.store.put(7, signed(7, host='validator-1'))
        self.store.put(8, signed(8, chain='dytallix-staging-2'))
        self.store.put(9, signed(9, not_after=self.now - 1))
        self.store.put(10, signed(10, not_after=self.now + agent.MAX_VALIDITY_SECONDS + 1))
        self.store.put(11, signed(11, 'journal', {'lines': 50_000}))
        self.store.put(12, signed(12, 'journal', {'lines': 20}))
        self.store.put(13, signed(13, 'fault-netem', {'delay_ms': 100, 'loss_percent': 10, 'duration_seconds': 30}))
        summary = self.poll()
        self.assertEqual(summary['next_sequence'], 14)
        self.assertEqual(summary['errors'], [])
        results = self.results()
        self.assertEqual(sorted(results), list(range(1, 14)))
        expected = {1: 'DONE', 12: 'DONE', 13: 'DONE'}
        self.assertEqual({n: r['status'] for n, r in results.items()},
                         {n: expected.get(n, 'REFUSED') for n in range(1, 14)})
        # fault-netem built its argv from the signed arguments and scheduled its own dead-man clear.
        self.assertIn(['tc', 'qdisc', 'replace', 'dev', 'eth0', 'root', 'netem', 'delay', '100ms', 'loss', '10%'],
                      self.host.commands)
        self.assertIn(['systemd-run', '--on-active=30s', 'tc', 'qdisc', 'del', 'dev', 'eth0', 'root'],
                      self.host.commands)
        reasons = {n: r['reason'] for n, r in results.items()}
        self.assertIn('does not verify', reasons[2])
        self.assertIn('signed as sequence 1, expected 3', reasons[3])
        self.assertIn('shell is not on the allowlist', reasons[4])
        self.assertIn('not a signed command', reasons[5])
        self.assertIn('does not verify', reasons[6])
        self.assertIn('signed for host validator-1', reasons[7])
        self.assertIn('signed for chain dytallix-staging-2', reasons[8])
        self.assertEqual(reasons[9], 'expired')
        self.assertIn('more than 24 hours', reasons[10])
        self.assertIn('outside 1 to 5000', reasons[11])
        # Only the allowed commands ran: status's steps and journal's.
        ran = [' '.join(c[:3]) for c in self.host.commands if c[0] != 'systemctl' or c[1] != 'enable']
        self.assertIn('systemctl is-active dytallix-node.service', ran)
        self.assertIn(f'{BIN}/dytallix-operator-rpc --home /var/lib/dytallix/node', ran)
        self.assertIn(['journalctl', '-u', 'dytallix-node.service', '-o', 'cat', '--no-pager', '-n', '20'],
                      self.host.commands)
        self.assertFalse([c for c in self.host.commands if 'stop' in c])
        self.assertEqual(results[1]['command_sha256'], hashlib.sha256(signed(1, 'status')).hexdigest())
        self.assertEqual(results[1]['steps'][0]['stdout'], 'active\n')
        # Nothing is left waiting, and the next poll finds nothing new.
        self.assertEqual(list(self.host.path(f'{agent.STATE}/outbox').iterdir()), [])
        self.assertEqual(self.poll()['handled'], [])

    def test_fault_partition_drops_the_p2p_port_both_ways_and_self_expires(self):
        self.store.put(1, signed(1, 'fault-partition', {'duration_seconds': 30}))
        self.poll()
        self.assertEqual(self.results()[1]['status'], 'DONE')
        cmds = self.host.commands
        self.assertIn(['nft', 'add', 'element', 'inet', 'dytallix_fault', 'blocked', '{ 26656 timeout 30s }'], cmds)
        for chain, match in (('out', 'dport'), ('out', 'sport'), ('in', 'dport'), ('in', 'sport')):
            self.assertIn(['nft', 'add', 'rule', 'inet', 'dytallix_fault', chain, 'tcp', match, '@blocked', 'drop'], cmds)

    def test_a_slot_is_spent_before_its_command_runs(self):
        self.store.put(1, signed(1, 'stop'))
        self.store.fail_uploads = True
        summary = self.poll()
        self.assertEqual(summary['handled'][0]['status'], 'DONE')
        self.assertTrue(summary['errors'])
        # The result waits; the command never runs twice.
        self.assertEqual(len(list(self.host.path(f'{agent.STATE}/outbox').iterdir())), 1)
        self.store.fail_uploads = False
        summary = self.poll()
        self.assertEqual(summary['handled'], [])
        self.assertEqual(len(summary['uploaded']), 1)
        self.assertEqual(sum(c == ['systemctl', 'stop', 'dytallix-node.service'] for c in self.host.commands), 1)

    def test_store_failures_spend_nothing_and_oversized_commands_are_refused(self):
        self.store.status = 503
        with self.assertRaisesRegex(agent.Failed, 'answered 503'):
            self.poll()
        self.assertFalse(self.host.path(f'{agent.STATE}/state.json').exists())
        self.store.status = None
        self.store.put(1, b'{' + b' ' * agent.MAX_COMMAND_BYTES + b'}')
        summary = self.poll()
        self.assertEqual(summary['handled'][0]['status'], 'REFUSED')
        self.assertIn('larger than', summary['handled'][0]['reason'])
        self.assertEqual(summary['next_sequence'], 2)

    def test_state_for_another_chain_never_restarts_the_sequence(self):
        self.store.put(1, signed(1))
        self.assertEqual(self.poll()['next_sequence'], 2)
        state = json.loads(self.host.path(f'{agent.STATE}/state.json').read_bytes())
        state['chain_id'] = 'dytallix-staging-0'
        self.host.path(f'{agent.STATE}/state.json').write_text(json.dumps(state))
        with self.assertRaisesRegex(agent.Failed, 'reinstall the agent'):
            self.poll()


class ParsingTests(unittest.TestCase):
    def check(self, raw, expected=1, now=1_900_000_000):
        return agent.check_command(raw, settings(), ALLOWLIST, expected, now, fake_verify)

    def test_strict_json(self):
        good = json.loads(signed(1))
        cases = {
            'appears twice': signed(1).replace(b'"body"', b'"body": {}, "body"', 1),
            'not allowed': signed(1).replace(b'"sequence": 1', b'"sequence": 1.0'),
            'not JSON': b'\xff',
            'not a command': json.dumps({'body': dict(good['body'], extra=1), 'signature': good['signature']}).encode(),
            'malformed sequence': json.dumps({'body': dict(good['body'], sequence=True),
                                              'signature': good['signature']}).encode(),
            'malformed arguments': json.dumps({'body': dict(good['body'], args={'lines': '20'}),
                                               'signature': good['signature']}).encode(),
            'lowercase hex': json.dumps({'body': good['body'], 'signature': good['signature'].upper()}).encode(),
        }
        for reason, raw in cases.items():
            with self.assertRaisesRegex(agent.Refused, reason):
                self.check(raw)
        self.assertEqual(self.check(signed(1))[0]['command'], 'status')

    def test_the_signed_message_is_the_canonical_body(self):
        body = staging_command.body(CHAIN, LABEL, 1, 'status', {}, 5)
        self.assertEqual(agent.canonical(body),
                         b'{"args":{},"chain_id":"dytallix-staging-1","command":"status","host":"sentry-1",'
                         b'"not_after":5,"schema":"dytallix.staging-command.v1","sequence":1}')
        self.assertEqual(agent.message(body), agent.DOMAIN + hashlib.sha256(agent.canonical(body)).hexdigest())

    def test_the_allowlist_runs_no_shell_and_has_no_stubs(self):
        for name, entry in ALLOWLIST['commands'].items():
            for step in entry['steps']:
                self.assertNotIn(Path(step[0]).name, ('sh', 'bash', 'dash', 'python3', 'env'), name)
            if entry.get('stub'):
                self.assertEqual(entry['steps'], [], name)
        # The T5 fault commands are built; nothing is a stub any more.
        self.assertEqual([n for n, e in ALLOWLIST['commands'].items() if e.get('stub')], [])
        bad = json.loads(json.dumps(ALLOWLIST))
        bad['commands']['journal']['steps'][0].append('{unknown}')
        with self.assertRaisesRegex(agent.Failed, 'unknown placeholder'):
            agent.check_allowlist(bad)

    def test_settings_refuse_unsafe_keys(self):
        for change, reason in (({'command_public_key_hex': 'ab'}, 'ML-DSA-65 public key'),
                               ({'commands': dict(STORE, endpoint='http://store.example')}, 'https'),
                               ({'commands': dict(STORE, secret_access_key='a"b')}, 'unexpected characters'),
                               ({'label': '../x'}, 'not a name')):
            with self.assertRaisesRegex(agent.Failed, reason):
                agent.check_settings(dict(settings(), **change))


class FounderTests(unittest.TestCase):
    def test_make_checks_the_allowlist_before_signing(self):
        calls = []
        with self.assertRaisesRegex(staging_command.Failed, 'not on the allowlist'):
            staging_command.make('cli', 'home', 'key.json', CHAIN, LABEL, 1, 'shell', {}, 60, 'out',
                                 run=lambda *a, **k: calls.append(a))
        with self.assertRaisesRegex(staging_command.Failed, 'outside 1 to 5000'):
            staging_command.make('cli', 'home', 'key.json', CHAIN, LABEL, 1, 'journal', {'lines': 0}, 60, 'out',
                                 run=lambda *a, **k: calls.append(a))
        with self.assertRaisesRegex(staging_command.Failed, '1 minute to 24 hours'):
            staging_command.make('cli', 'home', 'key.json', CHAIN, LABEL, 1, 'status', {}, 24 * 60 + 1, 'out')
        self.assertEqual(calls, [])

    def test_arguments_and_the_object_key(self):
        self.assertEqual(staging_command.parse_args(['lines=20']), {'lines': 20})
        for bad in (['lines'], ['lines=x'], ['lines=1', 'lines=2'], ['Lines=1']):
            with self.assertRaises(staging_command.Failed):
                staging_command.parse_args(bad)
        body = staging_command.body(CHAIN, LABEL, 7, 'status', {}, 5)
        self.assertEqual(staging_command.object_key(body), f'{CHAIN}/{LABEL}/0000000007.json')

    def test_sign_uses_the_cli_and_checks_its_own_signature(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            (home / '.dytallix').mkdir()
            (home / '.dytallix' / 'keystore.json').write_text(json.dumps({
                'active': staging_command.WALLET,
                'entries': [{'name': staging_command.WALLET, 'public_key': list(bytes.fromhex(KEY))}]}))
            calls = []

            def run(argv, env=None, capture_output=True, text=True):
                calls.append(argv[1:3])
                if argv[2] == 'sign':
                    return subprocess.CompletedProcess(argv, 0, f'Signature\n{fake_signature(KEY, argv[3])}\n', '')
                return subprocess.CompletedProcess(argv, 0 if fake_verify(*argv[3:6]) else 1, '', '')
            body = staging_command.body(CHAIN, LABEL, 1, 'status', {}, 1_900_003_600)
            raw = staging_command.sign('cli', home, KEY, body, run)
            self.assertEqual(calls, [['crypto', 'sign'], ['crypto', 'verify']])
            self.assertEqual(agent.check_command(raw, settings(), ALLOWLIST, 1, 1_900_000_000, fake_verify)[0], body)
            with self.assertRaisesRegex(staging_command.Failed, 'not the key record'):
                staging_command.sign('cli', home, OTHER_KEY, body, run)

    def test_package_is_deterministic(self):
        with tempfile.TemporaryDirectory() as tmp:
            first = staging_command.package(Path(tmp) / 'a')
            self.assertEqual(first, staging_command.package(Path(tmp) / 'b'))


if __name__ == '__main__':
    unittest.main()
