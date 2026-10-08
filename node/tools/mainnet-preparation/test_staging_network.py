"""The three-host staging network's pieces that run without KVM (H4): the
plan, the VMs' cloud-init, the guest agent client against a stand-in agent,
and the stand-in store. The whole network runs in the Host install
workflow."""
import base64
import json
import socket
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from http.server import ThreadingHTTPServer
from pathlib import Path

import s3_standin
import staging_network as sn


class PlanTests(unittest.TestCase):
    def test_each_host_has_its_own_address(self):
        plan = sn.network_plan()
        self.assertEqual(plan['chain_id'], 'dytallix-staging-1')
        hosts = {h['label']: h for h in plan['hosts']}
        self.assertEqual(hosts['validator-1']['p2p'], '10.77.0.1:26656')
        self.assertEqual(hosts['sentry-1']['p2p'], '10.77.0.11:26656')
        self.assertEqual((hosts['endpoint-1']['channel'], hosts['endpoint-1']['status']),
                         ('10.77.0.12:26670', '10.77.0.12:8080'))

    def test_the_paths_match_the_host_files(self):
        import host_files
        self.assertEqual((sn.SNAPSHOTS, sn.SNAPSHOT_LIGHT_BLOCKS), (host_files.SNAPSHOTS, host_files.SNAPSHOT_LIGHT_BLOCKS))

    def test_cloud_init_gives_the_vm_its_address_and_the_agent(self):
        network = sn.network_config('sentry-1').decode()
        self.assertIn('addresses: [10.77.0.11/24]', network)
        self.assertIn('52:54:00:77:00:11', network)
        self.assertIn('via: 10.77.0.1', network)
        user = sn.user_data('sentry-1').decode()
        self.assertTrue(user.startswith('#cloud-config\n'))
        self.assertIn('qemu-guest-agent', user)
        self.assertIn('ufw disable', user)


class FakeAgent(threading.Thread):
    """A guest agent on a UNIX socket: guest-sync, guest-exec (echoes its
    standard input), guest-exec-status and guest-file reads."""

    def __init__(self, path, files):
        super().__init__(daemon=True)
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(path))
        self.server.listen()
        self.files, self.exec_input = files, {}

    def run(self):
        while True:
            try:
                connection, _ = self.server.accept()
            except OSError:
                return
            with connection, connection.makefile('rwb') as stream:
                for line in stream:
                    request = json.loads(line)
                    reply = self.answer(request['execute'], request.get('arguments', {}))
                    stream.write(json.dumps(reply).encode() + b'\n')
                    stream.flush()

    def answer(self, command, arguments):
        if command == 'guest-sync':
            return {'return': arguments['id']}
        if command == 'guest-ping':
            return {'return': {}}
        if command == 'guest-exec':
            self.exec_input[7] = (arguments['path'], base64.b64decode(arguments.get('input-data', '')))
            return {'return': {'pid': 7}}
        if command == 'guest-exec-status':
            path, stdin = self.exec_input[arguments['pid']]
            code = 3 if path == 'false' else 0
            return {'return': {'exited': True, 'exitcode': code, 'out-data': base64.b64encode(stdin).decode()}}
        if command == 'guest-file-open':
            if arguments['path'] not in self.files:
                return {'error': {'desc': 'no such file'}}
            self.reading = bytearray(self.files[arguments['path']])
            return {'return': 1000}
        if command == 'guest-file-read':
            chunk, self.reading = self.reading[:arguments['count']], self.reading[arguments['count']:]
            return {'return': {'buf-b64': base64.b64encode(bytes(chunk)).decode(), 'eof': not self.reading}}
        if command == 'guest-file-close':
            return {'return': {}}
        return {'error': {'desc': f'unknown {command}'}}


class AgentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir='/tmp')
        self.addCleanup(self.tmp.cleanup)
        path = Path(self.tmp.name) / 'qga'
        self.fake = FakeAgent(path, {'/var/x': b'y' * 3_000_000})
        self.fake.start()
        self.addCleanup(self.fake.server.close)
        self.agent = sn.Agent(path)

    def test_runs_commands_with_standard_input_and_reads_files(self):
        self.agent.wait(5)
        code, out, _ = self.agent.run(['install.sh'], stdin='dytallix-seal-sentry-1 0000\n')
        self.assertEqual((code, out), (0, 'dytallix-seal-sentry-1 0000\n'))
        with self.assertRaisesRegex(sn.Failed, 'exited 3'):
            self.agent.run(['false'])
        self.assertEqual(self.agent.run(['false'], check=False)[0], 3)
        self.assertEqual(self.agent.read('/var/x'), b'y' * 3_000_000)
        with self.assertRaisesRegex(sn.Failed, 'no such file'):
            self.agent.read('/var/none')


class StandinTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), s3_standin.handler(self.tmp.name, 'STANDIN'))
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.shutdown)
        self.base = f'http://127.0.0.1:{self.server.server_address[1]}'

    def put(self, path, body, authorization):
        request = urllib.request.Request(self.base + path, data=body, method='PUT',
                                         headers={'Authorization': authorization} if authorization else {})
        with urllib.request.urlopen(request, timeout=5) as response:
            return response.status

    def test_stores_only_signed_puts(self):
        signed = 'AWS4-HMAC-SHA256 Credential=STANDIN/20261008/auto/s3/aws4_request, SignedHeaders=host, Signature=ab'
        self.assertEqual(self.put('/copies/staging/chain/1.bin', b'copy', signed), 200)
        self.assertEqual((Path(self.tmp.name) / 'copies/staging/chain/1.bin').read_bytes(), b'copy')
        for authorization in (None, 'AWS4-HMAC-SHA256 Credential=OTHER/x'):
            with self.assertRaises(urllib.error.HTTPError) as refused:
                self.put('/copies/x.bin', b'copy', authorization)
            self.assertEqual(refused.exception.code, 403)
        with self.assertRaises(urllib.error.HTTPError):
            self.put('/copies/../escape.bin', b'copy', signed)
        # An object is never replaced.
        with self.assertRaises(urllib.error.HTTPError) as refused:
            self.put('/copies/staging/chain/1.bin', b'other', signed)
        self.assertEqual(refused.exception.code, 409)


if __name__ == '__main__':
    unittest.main()
