"""The founder's side of the staging agent (T2, the real-host harness): the
store's S3 calls through the real curl against the stand-in, the channel's
sequencing and result checks, the status page reader, and the fault
scenarios driven end to end through a fake channel and status page."""
from datetime import datetime, timezone
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import faults  # noqa: E402
import s3_standin  # noqa: E402
import staging_founder as sf  # noqa: E402

FOUNDER, READ = 'FOUNDERKEY', 'READKEY'


def store_record(port, **changes):
    record = {'schema': sf.STORE_SCHEMA, 'endpoint': f'http://127.0.0.1:{port}', 'bucket': 'copies',
              'region': 'auto', 'access_key_id': FOUNDER, 'secret_access_key': 'c2VjcmV0',
              'commands_prefix': 'commands/', 'results_prefix': 'history/'}
    record.update(changes)
    return record


class Standin:
    def __init__(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / 'root'
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), s3_standin.handler(str(self.root), ['UPLOADKEY'], READ,
                                                                                FOUNDER))
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()


class StoreTests(unittest.TestCase):
    def setUp(self):
        self.standin = Standin()
        self.addCleanup(self.standin.close)
        self.scratch = Path(self.standin.tmp.name) / 'scratch'
        self.store = sf.Store(sf.check_store(store_record(self.standin.port), standin=True), self.scratch)

    def test_put_get_list_through_curl(self):
        self.store.put('commands/c/h/0000000001.json', b'one')
        self.store.put('commands/c/h/0000000002.json', b'two')
        self.store.put('history/c/agent/h/x.bin', b'three')
        self.assertEqual(self.store.get('commands/c/h/0000000002.json'), (200, b'two'))
        self.assertEqual(self.store.get('commands/c/h/0000000009.json'), (404, None))
        self.assertEqual(self.store.list('commands/c/h/'), ['commands/c/h/0000000001.json', 'commands/c/h/0000000002.json'])
        self.assertEqual(self.store.list('nothing/'), [])

    def test_a_command_is_never_replaced(self):
        self.store.put('commands/c/h/0000000001.json', b'one')
        with self.assertRaisesRegex(sf.Failed, 'never replace'):
            self.store.put('commands/c/h/0000000001.json', b'other')
        self.assertEqual(self.store.get('commands/c/h/0000000001.json'), (200, b'one'))

    def test_a_host_read_key_cannot_list_or_put(self):
        reader = sf.Store(sf.check_store(store_record(self.standin.port, access_key_id=READ), standin=True), self.scratch)
        with self.assertRaisesRegex(sf.Failed, 'HTTP 403'):
            reader.list('commands/')
        with self.assertRaisesRegex(sf.Failed, '403'):
            reader.put('commands/c/h/0000000001.json', b'one')

    def test_plain_http_only_to_the_standin(self):
        with self.assertRaisesRegex(sf.Failed, 'https'):
            sf.check_store(store_record(1))
        sf.check_store(store_record(1, endpoint='https://s3.us-west-004.backblazeb2.com'))
        for bad in ({'commands_prefix': 'commands'}, {'bucket': 'a b'}, {'endpoint': 'https://x/path'}):
            with self.assertRaises(sf.Failed, msg=bad):
                sf.check_store(store_record(1, **bad), standin=True)
        extra = store_record(1)
        extra['more'] = 1
        with self.assertRaisesRegex(sf.Failed, 'extra'):
            sf.check_store(extra, standin=True)


SIGNER = '''#!/usr/bin/env python3
import sys
args = dict(zip(sys.argv[2::2], sys.argv[3::2]))
open(args['-out'], 'wb').write(open(args['-in'], 'rb').read())
'''


class ChannelTests(unittest.TestCase):
    def setUp(self):
        self.standin = Standin()
        self.addCleanup(self.standin.close)
        base = Path(self.standin.tmp.name)
        self.store = sf.Store(sf.check_store(store_record(self.standin.port), standin=True), base / 'scratch')
        key = os.urandom(1952).hex()
        self.key_path = base / 'key.json'
        self.key_path.write_text(json.dumps({'schema': sf.staging_command.KEY_SCHEMA, 'public_key_hex': key,
                                             'key_id': hashlib.sha256(bytes.fromhex(key)).hexdigest()}))
        self.signer = base / 'signer.py'
        self.signer.write_text(SIGNER)
        self.signer.chmod(0o755)
        self.paper = base / 'paper.txt'
        self.paper.write_text('code\n')
        signed = mock.patch.object(sf.staging_command, 'sign',
                                   lambda cli, home, key, body: json.dumps({'body': body, 'signature': 'ab'}).encode())
        signed.start()
        self.addCleanup(signed.stop)
        self.channel = sf.Channel(self.store, 'chain-1', 'sentry-1', base / 'results', 'cli', 'home', self.key_path,
                                  self.paper, self.signer, sleep=lambda s: None, say=lambda m: None)

    def result(self, sequence, command_sha256, **changes):
        result = {'schema': 'x', 'chain_id': 'chain-1', 'host': 'sentry-1', 'sequence': sequence,
                  'command_sha256': command_sha256, 'command': 'status', 'args': {}, 'status': 'DONE',
                  'reason': None, 'steps': [], 'finished_at': 100}
        result.update(changes)
        raw = json.dumps(result).encode()
        name = f'history/chain-1/agent/sentry-1/{sequence:010}-{hashlib.sha256(raw).hexdigest()}.bin'
        self.store.put(name, raw)
        return name

    def test_send_takes_the_slot_after_the_highest_object(self):
        # Two planted (or refused) objects: the founder never reuses their slots.
        self.store.put('commands/chain-1/sentry-1/0000000001.json', b'planted')
        self.store.put('commands/chain-1/sentry-1/0000000005.json', b'planted')
        self.store.put('commands/chain-1/validator-1/0000000009.json', b'another host')
        self.assertEqual(self.channel.send('status'), 6)
        status, raw = self.store.get('commands/chain-1/sentry-1/0000000006.json')
        body = json.loads(raw)['body']
        self.assertEqual((body['host'], body['chain_id'], body['sequence'], body['command']),
                         ('sentry-1', 'chain-1', 6, 'status'))

    def test_send_refuses_what_the_agent_would(self):
        with self.assertRaisesRegex(sf.staging_command.Failed, 'not on the allowlist'):
            self.channel.send('shell')
        with self.assertRaisesRegex(sf.staging_command.Failed, 'outside'):
            self.channel.send('journal', {'lines': 0})

    def test_wait_opens_and_binds_the_result(self):
        sequence = self.channel.send('status')
        self.result(sequence, self.channel.sent[sequence])
        self.assertEqual(self.channel.wait([sequence])[0]['sequence'], sequence)

    def test_a_result_for_another_command_file_is_refused(self):
        sequence = self.channel.send('status')
        self.result(sequence, '0' * 64)
        with self.assertRaisesRegex(sf.Failed, 'another command file'):
            self.channel.wait([sequence])

    def test_a_result_for_another_host_or_a_bad_copy_is_refused(self):
        self.result(1, None, host='validator-1')
        with self.assertRaisesRegex(sf.Failed, 'is a result for validator-1'):
            self.channel.open_all()
        channel = sf.Channel(self.store, 'chain-1', 'sentry-1', Path(self.standin.tmp.name) / 'r2',
                             paper=self.paper, signer=self.signer)
        self.store.put(f'history/chain-1/agent/sentry-1/0000000002-{"0" * 64}.bin', b'{}')
        with self.assertRaisesRegex(sf.Failed, 'does not match its name'):
            channel.opened[1] = {}
            channel.open_all()

    def test_wait_times_out(self):
        clock = iter(range(0, 10_000, 100))
        channel = sf.Channel(self.store, 'chain-1', 'sentry-1', Path(self.standin.tmp.name) / 'r3',
                             paper=self.paper, signer=self.signer, clock=lambda: next(clock), sleep=lambda s: None)
        with self.assertRaisesRegex(sf.Failed, 'no result for sequence'):
            channel.wait([1], timeout=500)


class StatusPageTests(unittest.TestCase):
    def serve(self, code, body):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                raw = body.encode()
                self.send_response(code)
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def log_message(self, *args):
                pass
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return f'http://127.0.0.1:{server.server_address[1]}/status'

    def test_200_and_503_carry_the_fields(self):
        page = {'chain_id': 'c', 'height': '12', 'time': '2026-10-14T12:00:00.123456789Z'}
        ok = sf.status_page(self.serve(200, json.dumps(page)))
        self.assertEqual((ok['http'], ok['height'], ok['chain_id']), (200, 12, 'c'))
        stale = sf.status_page(self.serve(503, json.dumps(page)))
        self.assertEqual((stale['http'], stale['height']), (503, 12))

    def test_no_answer_and_garbage(self):
        self.assertIsNone(sf.status_page('http://127.0.0.1:9/status', timeout=1)['http'])
        self.assertNotIn('height', sf.status_page(self.serve(200, 'not json')))

    def test_block_instant(self):
        self.assertEqual(sf.block_instant({'time': '2026-10-14T12:00:00.5Z'}),
                         datetime(2026, 10, 14, 12, 0, 0, 500000, tzinfo=timezone.utc).timestamp())
        self.assertEqual(sf.block_instant({'time': '2026-10-14T12:00:00.123456789Z'}),
                         datetime(2026, 10, 14, 12, 0, 0, 123456, tzinfo=timezone.utc).timestamp())
        self.assertIsNone(sf.block_instant({'time': 'yesterday'}))
        self.assertIsNone(sf.block_instant({}))


class FakeChannel:
    """Runs each command at once: the fault takes hold, its proof comes back."""

    def __init__(self, page):
        self.page, self.sequence, self.results, self.commands = page, 0, {}, []

    def send(self, command, args=None):
        self.sequence += 1
        self.commands.append(command)
        now = time.time()
        proof = {'fault-netem': 'qdisc netem 8001: root refcnt 2 limit 1000 delay 400ms loss 5%',
                 'fault-partition': 'set blocked { type inet_service elements = { 26656 expires 1s } }',
                 'fault-kill': 'failed', 'start': 'active', 'fault-clear': ''}[command]
        if command == 'fault-kill':
            self.page.down = True
        if command == 'start':
            self.page.down = False
        if command in ('fault-netem', 'fault-partition'):
            self.page.stalled_until = now + args['duration_seconds']
        self.results[self.sequence] = {'sequence': self.sequence, 'status': 'DONE', 'finished_at': now,
                                       'steps': [{'stdout': proof}]}
        return self.sequence

    def wait(self, sequences, timeout=600):
        return [self.results[s] for s in sequences]

    def run(self, command, args=None, timeout=600):
        return self.wait([self.send(command, args)])[0]


class FakePage:
    """A sentry whose height follows the clock, except while stalled or down."""

    def __init__(self):
        self.height, self.down, self.stalled_until, self.last = 100, False, 0, time.time()

    def read(self):
        now = time.time()
        if self.down:
            return {'http': None, 'read_at': sf.iso(now)}
        if now >= self.stalled_until and now - self.last >= 0.05:
            self.height, self.last = self.height + 1, now
        made = datetime.fromtimestamp(self.last, tz=timezone.utc).strftime('%Y-%m-%dT%H:%M:%S.%fZ')
        return {'http': 200, 'chain_id': 'c', 'height': self.height, 'time': made, 'read_at': sf.iso(now)}


class ScenarioTests(unittest.TestCase):
    SCENARIOS = (
        {'name': 'latency', 'command': 'fault-netem', 'args': {'delay_ms': 400, 'loss_percent': 5, 'duration_seconds': 1}},
        {'name': 'partition', 'command': 'fault-partition', 'args': {'duration_seconds': 1}},
        {'name': 'process_kill', 'command': 'fault-kill', 'args': {}},
    )

    def test_the_scenarios_pass_through_the_public_paths(self):
        page = FakePage()
        channel = FakeChannel(page)
        cleared = []
        rows = faults.run_scenarios(channel, page.read, sf.block_instant, self.SCENARIOS, result_timeout=5,
                                    recovery_timeout=10, say=lambda m: None, period=0.02, on_cleared=cleared.append)
        record = faults.build_run('c', b'{}', b'{}', rows, 120, 'sentry-1')
        self.assertEqual([r['status'] for r in record['scenarios']], ['PASS'] * 3, record['scenarios'])
        self.assertEqual(faults.verify(json.loads(json.dumps(record)), b'{}', b'{}'), 'PASS')
        self.assertEqual(cleared, ['latency', 'partition'])
        # The kill and its restart go out together; each network fault is cleaned up.
        self.assertEqual(channel.commands, ['fault-netem', 'fault-clear', 'fault-partition', 'fault-clear',
                                            'fault-kill', 'start'])
        for row in record['scenarios']:
            self.assertEqual(row['clocks'], faults.CLOCKS)
            self.assertGreater(row['recovered_height'], row['height_at_clear'])

    def test_a_sentry_that_never_recovers_fails(self):
        page = FakePage()
        channel = FakeChannel(page)
        send = channel.send

        def stuck(command, args=None):
            sequence = send(command, args)
            page.stalled_until = time.time() + 3600  # the fault never lets go
            return sequence
        channel.send = stuck
        rows = faults.run_scenarios(channel, page.read, sf.block_instant, self.SCENARIOS[1:2], result_timeout=5,
                                    recovery_timeout=0.5, say=lambda m: None, period=0.02)
        record = faults.build_run('c', b'{}', b'{}', rows, 120, 'sentry-1')
        self.assertEqual(record['scenarios'][0]['status'], 'FAIL')
        self.assertIn('did not catch back up', record['scenarios'][0]['detail'])

    def test_the_proof_comes_from_the_last_step(self):
        netem = {'steps': [{'stdout': ''}, {'stdout': 'qdisc netem 8001: root'}]}
        self.assertTrue(faults.effect_holds('netem', faults.effect_of('latency', netem)))
        self.assertFalse(faults.effect_holds('netem', faults.effect_of('latency', {'steps': [{'stdout': 'qdisc fq_codel'}]})))
        self.assertTrue(faults.effect_holds('partition', faults.effect_of('partition', {'steps': [{'stdout': '{ 26656 }'}]})))
        self.assertFalse(faults.effect_holds('process', faults.effect_of('process_kill', {'steps': [{'stdout': 'active'}]})))
        self.assertFalse(faults.effect_holds('netem', faults.effect_of('latency', {'steps': []})))


if __name__ == '__main__':
    unittest.main()
