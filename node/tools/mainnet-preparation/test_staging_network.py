"""The three-host staging network's pieces that run without KVM (H4): the
plan, the VMs' cloud-init, the guest agent client against a stand-in agent,
the stand-in store, and the stand-in alerting service and helpers of the
monitoring drill (M5). The whole network runs in the Host install
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

import alert_standin
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

    def test_the_restore_drill_compares_height_and_both_hashes(self):
        raw = json.dumps({'jsonrpc': '2.0', 'id': -1, 'result': {'sync_info': {
            'latest_block_height': '41', 'latest_block_hash': 'AB', 'latest_app_hash': 'CD', 'catching_up': False}}})
        self.assertEqual(sn.sync_info(raw), (41, 'AB', 'CD'))
        with self.assertRaises(KeyError):
            sn.sync_info(json.dumps({'result': {}}))

    def test_a_swapped_signature_keeps_the_nodes_canonical_form(self):
        # The node decodes only its canonical form (compact, its key order),
        # so the kit drill's old-kit control must keep it to be refused for
        # its key rather than its encoding.
        control = (b'{"kind":"dytallix-emergency-control-v2","payload":{"schema":2,"chain_id":"c",'
                   b'"v2":{"authority_epoch":2,"resume":null}},"signatures":[{"key_id":"aa","signature_hex":"01"},'
                   b'{"key_id":"cc","signature_hex":"03"}]}')
        swapped = sn.with_signature(control, 'cc', {'key_id': 'bb', 'signature_hex': '02', 'schema': 'ignored'})
        self.assertEqual(swapped, control.replace(b'"key_id":"cc","signature_hex":"03"',
                                                  b'"key_id":"bb","signature_hex":"02"'))
        self.assertEqual(sn.with_signature(swapped, 'bb', {'key_id': 'cc', 'signature_hex': '03'}), control)
        # Sorted by key ID, as the node requires.
        resorted = sn.with_signature(control, 'aa', {'key_id': 'dd', 'signature_hex': '04'})
        self.assertEqual([s['key_id'] for s in json.loads(resorted)['signatures']], ['cc', 'dd'])
        with self.assertRaises(sn.Failed):
            sn.with_signature(control, 'ee', {'key_id': 'ff', 'signature_hex': '05'})

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



class AlertStandinTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.now = 1000.0
        self.service = alert_standin.Service(self.tmp.name, 180, clock=lambda: self.now)
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), alert_standin.handler(self.service))
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.addCleanup(self.server.shutdown)
        self.base = f'http://127.0.0.1:{self.server.server_address[1]}'

    def post(self, path, body):
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        request = urllib.request.Request(self.base + path, data=data, method='POST',
                                         headers={'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=5) as response:
            return response.status

    def events(self):
        return sn.parse_events((Path(self.tmp.name) / 'events.jsonl').read_text())

    def test_records_heartbeats_alerts_and_escalations(self):
        beat = {'schema': 'dytallix.heartbeat.v1', 'host': 'sentry-1', 'role': 'sentry', 'chain_id': 'c',
                'time': 999, 'firing': []}
        alert = {'schema': 'dytallix.alert.v1', 'condition': 'disk_low', 'status': 'firing', 'host': 'sentry-1',
                 'value': 3.1, 'threshold': 15}
        for path, body in (('/heartbeat', beat), ('/alert', alert), ('/escalation', alert)):
            self.assertEqual(self.post(path, body), 200)
        self.assertEqual([(e['to'], e['received']) for e in self.events()],
                         [('heartbeat', 1000.0), ('alert', 1000.0), ('escalation', 1000.0)])
        self.assertEqual(sn.find_event(self.events(), 'disk_low', 'firing', 0)[0], 1)
        # An escalation is not a change of state.
        self.assertIsNone(sn.find_event(self.events(), 'disk_low', 'firing', 2))
        for path, body in (('/other', beat), ('/alert', b'not json'), ('/alert', [1]), ('/alert', {'host': 3})):
            with self.assertRaises(urllib.error.HTTPError):
                self.post(path, body)
        self.assertEqual(len(self.events()), 3)

    def test_alerts_when_a_heartbeat_stops_and_resolves_when_it_returns(self):
        beat = {'host': 'sentry-1', 'role': 'sentry', 'chain_id': 'c', 'time': 1000}
        self.post('/heartbeat', beat)
        self.now += 179
        self.service.check()
        self.assertIsNone(sn.find_event(self.events(), 'monitoring_down', 'firing', 0))
        self.now += 1
        self.service.check()
        self.service.check()
        index, down = sn.find_event(self.events(), 'monitoring_down', 'firing', 0)
        self.assertEqual((down['to'], down['body']['severity'], down['body']['value'], down['body']['threshold'],
                          down['body']['role']), ('service', 1, 180, 180, 'sentry'))
        self.assertEqual(len(self.events()), 2)  # once, however many checks
        self.now += 60
        self.post('/heartbeat', beat)
        _, back = sn.find_event(self.events(), 'monitoring_down', 'resolved', index + 1)
        self.assertEqual(back['body']['value'], 240)
        self.now += 100
        self.service.check()
        self.assertEqual(len(self.events()), 4)


class MonitorDrillTests(unittest.TestCase):
    def test_the_fill_leaves_the_asked_free_space(self):
        # 1,000,000 blocks of 4096 bytes, 400,000 available: leave 3%.
        self.assertEqual(sn.fill_size('400000 1000000 4096', 3), 370000 * 4096)
        self.assertEqual(sn.fill_size('20000 1000000 4096', 3), 0)

    def test_finds_changes_of_state_in_order(self):
        events = [{'to': 'heartbeat', 'body': {'host': 'h'}},
                  {'to': 'alert', 'body': {'condition': 'consensus_halt', 'status': 'firing'}},
                  {'to': 'alert', 'body': {'condition': 'consensus_halt', 'status': 'resolved'}},
                  {'to': 'service', 'body': {'condition': 'monitoring_down', 'status': 'firing'}}]
        self.assertEqual(sn.find_event(events, 'consensus_halt', 'resolved', 0)[0], 2)
        self.assertIsNone(sn.find_event(events, 'consensus_halt', 'firing', 2))
        self.assertEqual(sn.find_event(events, 'monitoring_down', 'firing', 0)[0], 3)

    def test_the_approved_critical_level_is_above_the_fill(self):
        approved = {v['path']: v.get('approved') for v in json.loads(
            (Path(__file__).resolve().parents[3] / 'launch' / 'E05_VALUES.json').read_bytes())['values']}
        self.assertLess(sn.FILL_FREE_PERCENT, approved['monitor.disk_free_critical_percent'])
        # The alerting service's rule (monitoring v1): a heartbeat missing 3 minutes.
        self.assertEqual(sn.MONITORING_DOWN_SECONDS, 180)


if __name__ == '__main__':
    unittest.main()
