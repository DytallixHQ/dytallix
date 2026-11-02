"""The load machines (P01_T06_AGENT_RECONCILE_2026-10-20): the limits are in
code, every machine is destroyed whatever happens, and nothing overlaps. The
DigitalOcean API is a stand-in here; curl is real."""
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.parse import parse_qs, urlsplit

import load_machines as lm

TOKEN = 'dop_v1_' + 'ab' * 32


class FakeDigitalOcean:
    """The few endpoints the tool uses, with DigitalOcean's status codes."""

    def __init__(self):
        self.droplets, self.keys, self.next_id, self.calls = {}, {}, 100, []
        self.never_active, self.lock = False, threading.Lock()
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def reply(self, code, body=None):
                raw = json.dumps(body).encode() if body is not None else b''
                self.send_response(code)
                self.send_header('Content-Length', str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def route(self):
                if self.headers.get('Authorization') != f'Bearer {TOKEN}':
                    return self.reply(401, {'id': 'unauthorized'})
                split = urlsplit(self.path)
                query = {k: v[0] for k, v in parse_qs(split.query).items()}
                length = int(self.headers.get('Content-Length') or 0)
                body = json.loads(self.rfile.read(length)) if length else None
                with fake.lock:
                    fake.calls.append((self.command, split.path, query, body))
                    return self.reply(*fake.handle(self.command, split.path, query, body))
            do_GET = do_POST = do_DELETE = route

            def log_message(self, *args):
                pass
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = f'http://127.0.0.1:{self.server.server_address[1]}'

    def handle(self, method, path, query, body):
        if (method, path) == ('GET', '/v2/account'):
            return 200, {'account': {'status': 'active', 'droplet_limit': 10}}
        if (method, path) == ('GET', '/v2/regions'):
            return 200, {'regions': [{'slug': r, 'available': True, 'sizes': list(lm.SIZES)} for r in lm.REGIONS]}
        if (method, path) == ('POST', '/v2/account/keys'):
            self.next_id += 1
            self.keys[self.next_id] = body['name']
            return 201, {'ssh_key': {'id': self.next_id, 'fingerprint': 'f', 'name': body['name']}}
        if method == 'GET' and path == '/v2/account/keys':
            return 200, {'ssh_keys': [{'id': i, 'name': n} for i, n in self.keys.items()]}
        if method == 'DELETE' and path.startswith('/v2/account/keys/'):
            return (204, None) if self.keys.pop(int(path.rsplit('/', 1)[1]), None) else (404, {'id': 'not_found'})
        if (method, path) == ('POST', '/v2/droplets'):
            assert len(body['names']) <= lm.PER_REQUEST and body['size'] in lm.SIZES
            for name in body['names']:
                self.next_id += 1
                self.droplets[self.next_id] = {'id': self.next_id, 'name': name, 'status': 'new', 'tags': body['tags'],
                                               'region': {'slug': body['region']}, 'networks': {'v4': []}}
            return 202, {'droplets': []}
        if (method, path) == ('GET', '/v2/droplets'):
            found = [d for d in self.droplets.values() if query['tag_name'] in d['tags']]
            for d in found:
                if not self.never_active and d['status'] == 'new':
                    d['status'] = 'active'
                    d['networks']['v4'] = [{'type': 'private', 'ip_address': '10.0.0.1'},
                                           {'type': 'public', 'ip_address': f'203.0.113.{d["id"] % 250}'}]
            return 200, {'droplets': found, 'links': {}, 'meta': {'total': len(found)}}
        if (method, path) == ('DELETE', '/v2/droplets'):
            gone = [i for i, d in self.droplets.items() if query['tag_name'] in d['tags']]
            for i in gone:
                del self.droplets[i]
            return 204, None
        return 404, {'id': 'not_found'}

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class LoadMachinesTests(unittest.TestCase):
    def setUp(self):
        self.fake = FakeDigitalOcean()
        self.addCleanup(self.fake.close)
        self.api = lm.Api(TOKEN, self.fake.base, standin=True)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out = Path(self.tmp.name) / 'run'
        self.lines = []

    def up(self, count=4, **changes):
        return lm.up(self.api, self.out, count, changes.pop('hours', 2), sleep=lambda s: None,
                     say=self.lines.append, **changes)

    def test_up_spreads_the_machines_tags_them_and_down_destroys_them(self):
        watched = []
        state = self.up(4, watchdog=watched.append)
        self.assertEqual([m['region'] for m in state['machines']], list(lm.REGIONS[:4]))
        self.assertEqual(len({m['ipv4'] for m in state['machines']}), 4)
        self.assertTrue(all(lm.TAG in d['tags'] and state['tag'] in d['tags'] for d in self.fake.droplets.values()))
        self.assertEqual(watched, [self.out], 'the deadline watchdog starts once the key exists')
        self.assertEqual(oct(self.out.stat().st_mode & 0o777), '0o700')
        self.assertEqual(json.loads((self.out / lm.STATE).read_text())['machines'], state['machines'])
        created = [c for c in self.fake.calls if c[:2] == ('POST', '/v2/droplets')]
        self.assertTrue(all(c[3]['image'] == lm.IMAGE and c[3]['ssh_keys'] == [state['key_id']] for c in created))
        lm.down(self.api, self.out, say=self.lines.append)
        self.assertEqual((self.fake.droplets, self.fake.keys), ({}, {}))

    def test_the_limits_are_in_code(self):
        for count, hours, size, why in ((9, 2, lm.SIZES[0], 'machines'), (0, 2, lm.SIZES[0], 'machines'),
                                        (4, 13, lm.SIZES[0], 'hours'), (4, 2, 's-2vcpu-4gb', 'approved')):
            with self.subTest(why=why), self.assertRaisesRegex(lm.Failed, why):
                self.up(count, hours=hours, size=size)
        self.assertEqual(self.fake.droplets, {}, 'a refused plan rents nothing')

    def test_a_second_run_never_overlaps_a_machine_left_running(self):
        self.up(2)
        with self.assertRaisesRegex(lm.Failed, 'still exist'):
            lm.up(self.api, Path(self.tmp.name) / 'second', 2, 2, sleep=lambda s: None, say=self.lines.append)
        self.assertEqual(len(self.fake.droplets), 2)

    def test_a_failed_run_destroys_what_it_made(self):
        self.fake.never_active = True
        ticks = iter(range(0, 10_000, 100))
        with self.assertRaisesRegex(lm.Failed, 'only 0 of 3 machines active'):
            self.up(3, clock=lambda: next(ticks), timeout=500)
        self.assertEqual((self.fake.droplets, self.fake.keys), ({}, {}))

    def test_reap_removes_every_tagged_machine_and_this_tools_keys(self):
        self.up(3)
        self.fake.keys[999] = 'someone-else'
        self.assertEqual(lm.reap(self.api, say=self.lines.append), (3, 1))
        self.assertEqual((self.fake.droplets, self.fake.keys), ({}, {999: 'someone-else'}))

    def test_the_watchdog_destroys_the_run_at_its_deadline(self):
        state = self.up(2)
        deadline = datetime.strptime(state['deadline'], '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
        times = iter([deadline - timedelta(minutes=5), deadline - timedelta(seconds=1), deadline])
        slept = []
        lm.watch(self.api, self.out, clock=lambda: next(times), sleep=slept.append, say=self.lines.append)
        self.assertEqual(self.fake.droplets, {})
        self.assertEqual(slept, [60, 1])

    def test_the_token_stays_private_and_https_only(self):
        token = Path(self.tmp.name) / 'token'
        token.write_text(TOKEN + '\n')
        os.chmod(token, 0o644)
        with self.assertRaisesRegex(lm.Failed, 'owner only'):
            lm.load_token(token)
        os.chmod(token, 0o600)
        self.assertEqual(lm.load_token(token), TOKEN)
        token.write_text('not-a-token')
        with self.assertRaisesRegex(lm.Failed, 'no DigitalOcean personal access token'):
            lm.load_token(token)
        with self.assertRaisesRegex(lm.Failed, 'https'):
            lm.Api(TOKEN, 'http://api.example.com')
        with self.assertRaisesRegex(lm.Failed, 'HTTP 401'):
            lm.Api('dop_v1_' + '00' * 32, self.fake.base, standin=True).call('GET', '/v2/account')


if __name__ == '__main__':
    unittest.main()
