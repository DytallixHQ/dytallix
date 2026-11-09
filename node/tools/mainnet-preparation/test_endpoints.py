"""The endpoint and RPC family without a sentry: the outcome of each
observation, the record and its verification, and campaigns over real
sockets against stand-in listeners that apply the client channel's and the
status page's bounds and handshake checks (handshake.rs, channel.rs,
status.rs), including listeners that serve beyond a bound or answer a hello
they must refuse. The types that need several addresses use loopback
addresses of their own, which only Linux routes. The staging network runs
the family for real in the Host install workflow."""
import itertools
import json
import os
import socket
import sys
import threading
import time
import unittest
from unittest import mock

import endpoints

NETWORK = 'dytallix-staging-1'
KEY = bytes(range(256)) * 7 + bytes(160)
GENESIS, ENGINE = b'{"native": 1}', b'{"engine": 1}'
LINUX = sys.platform.startswith('linux')
FAST = {'channel_deadline': 1, 'page_deadline': 1}


def key_valid(encapsulation_key):
    """FIPS 203's input check: every 12-bit coefficient below q."""
    poly = encapsulation_key[:1152]
    for i in range(0, len(poly), 3):
        b0, b1, b2 = poly[i:i + 3]
        if (b0 | (b1 & 0x0f) << 8) >= 3329 or ((b1 >> 4) | b2 << 4) >= 3329:
            return False
    return True


class Listener:
    """A threaded listener with a total and a per-address bound: one over
    either is closed at once; each exchange ends at `deadline`."""

    def __init__(self, total, per_address, deadline):
        self.total, self.per_address, self.deadline = total, per_address, deadline
        self.server = socket.socket()
        self.server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.server.bind(('127.0.0.1' if not LINUX else '0.0.0.0', 0))
        self.server.listen(128)
        self.port = self.server.getsockname()[1]
        self.counts, self.open, self.lock = {}, 0, threading.Lock()
        threading.Thread(target=self.accept, daemon=True).start()

    def accept(self):
        while True:
            try:
                conn, (ip, _) = self.server.accept()
            except OSError:
                return
            with self.lock:
                full = self.open >= self.total or (self.per_address and self.counts.get(ip, 0) >= self.per_address)
                if not full:
                    self.open += 1
                    self.counts[ip] = self.counts.get(ip, 0) + 1
            if full:
                conn.close()
                continue
            threading.Thread(target=self.serve, args=(conn, ip), daemon=True).start()

    def serve(self, conn, ip):
        try:
            self.exchange(conn, time.monotonic() + self.deadline)
        except (OSError, ValueError, TimeoutError):
            pass
        finally:
            conn.close()
            with self.lock:
                self.open -= 1
                self.counts[ip] -= 1

    @staticmethod
    def read(conn, count, deadline):
        data = b''
        while len(data) < count:
            left = deadline - time.monotonic()
            if left <= 0:
                raise TimeoutError
            conn.settimeout(left)
            chunk = conn.recv(count - len(data))
            if not chunk:
                raise ConnectionError
            data += chunk
        return data

    def close(self):
        self.server.close()


class FakeChannel(Listener):
    """The endpoint's handshake checks (handshake.rs) in order. `lax`
    answers a well-formed hello for another network, key or a key that
    fails the FIPS 203 check; `mute` answers no hello."""

    def __init__(self, total=16, per_address=2, deadline=0.5, lax=False, mute=False):
        super().__init__(total, per_address, deadline)
        self.lax, self.mute = lax, mute

    def exchange(self, conn, deadline):
        header = self.read(conn, 8, deadline)
        if header[:4] != endpoints.MAGIC or header[4] != endpoints.VERSION or header[5] != endpoints.HELLO:
            return
        length = int.from_bytes(header[6:8], 'big')
        if not endpoints.HELLO_FIXED + 1 <= length <= endpoints.HELLO_FIXED + 64:
            return
        hello = self.read(conn, length, deadline)
        n = hello[0]
        if not 1 <= n <= 64 or len(hello) != endpoints.HELLO_FIXED + n:
            return
        key = hello[1 + n + endpoints.NONCE:1 + n + endpoints.NONCE + endpoints.PUBLIC_KEY]
        valid = hello[1:1 + n] == NETWORK.encode() and key == KEY and key_valid(hello[-endpoints.ENCAPSULATION_KEY:])
        if self.mute or not (valid or self.lax):
            return
        conn.sendall(endpoints.frame(endpoints.OFFER, os.urandom(endpoints.OFFER_LEN)))
        self.read(conn, 8 + 32, deadline)  # a finish no client here can confirm


class FakePage(Listener):
    """The status page: 16 headers and 8 KiB, else 431; `lax` has no
    header bounds."""

    def __init__(self, total=8, per_address=2, deadline=0.5, lax=False):
        super().__init__(total, per_address, deadline)
        self.lax, self.heights = lax, itertools.count(100)

    def exchange(self, conn, deadline):
        data, most = b'', (1 << 30) if self.lax else endpoints.LIMITS['page_header_bytes']
        while b'\r\n\r\n' not in data:
            if len(data) > most:
                conn.sendall(b'HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\n\r\n')
                return
            left = deadline - time.monotonic()
            if left <= 0:
                return
            conn.settimeout(left)
            chunk = conn.recv(4096)
            if not chunk:
                return
            data += chunk
        lines = data.split(b'\r\n\r\n')[0].split(b'\r\n')
        if not self.lax and len(lines) - 1 > endpoints.LIMITS['page_headers']:
            conn.sendall(b'HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\n\r\n')
            return
        body = json.dumps({'chain_id': NETWORK, 'height': next(self.heights), 'time': 't'}).encode()
        conn.sendall(b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: %d\r\n'
                     b'Connection: close\r\n\r\n' % len(body) + body)


class Holder:
    """Places.hold's process, as a thread."""

    def __init__(self, plan):
        self.ready_event, self.released = threading.Event(), threading.Event()
        self.count, self.report = 0, None
        self.thread = threading.Thread(target=self.go, args=(plan,), daemon=True)
        self.thread.start()

    def go(self, plan):
        socks = []
        try:
            socks = [endpoints.connect(plan) for _ in range(plan['count'])]
        except OSError:
            pass
        self.count = len(socks)
        self.ready_event.set()
        self.released.wait(plan.get('seconds', 8))
        self.report = {'held': len(socks), 'open': sum(map(endpoints.is_open, socks))}
        for sock in socks:
            sock.close()


class FakePlaces(endpoints.Places):
    """Each place is a loopback address of its own; the runner is
    127.0.0.1."""

    def __init__(self, sources=()):
        super().__init__([space for space, _ in sources])
        self.sources = dict(sources)

    def plan(self, space, plan):
        return dict(plan, source=self.sources.get(space, '127.0.0.1' if LINUX else None))

    def run(self, space, plan):
        return endpoints.probe(self.plan(space, plan))

    def hold(self, space, plan):
        return Holder(self.plan(space, plan))

    @staticmethod
    def ready(process, timeout=15):
        process.ready_event.wait(timeout)
        return process.count

    @staticmethod
    def release(process):
        process.released.set()
        process.thread.join(30)
        return process.report


def spaces(n):
    return [(f'ns{k}', f'127.0.0.{k + 2}') for k in range(n)]


def heights():
    counter = itertools.count(10)
    return lambda: {'channel_height': next(counter), 'page_status': 200, 'page_height': 5}


class Fast(unittest.TestCase):
    """Short deadlines, so stalled connections take about a second."""

    def setUp(self):
        for patch in (mock.patch.dict(endpoints.LIMITS, FAST), mock.patch.object(endpoints, 'AT_ONCE', 0.5),
                      mock.patch.object(endpoints, 'SLACK', 0.5)):
            patch.start()
            self.addCleanup(patch.stop)
        self.listeners = []

    def tearDown(self):
        for listener in self.listeners:
            listener.close()

    def listen(self, listener):
        self.listeners.append(listener)
        return ('127.0.0.1', listener.port)

    def campaign(self, channel, page, places=None, attempts=2):
        return endpoints.Campaign(places or FakePlaces(), channel, page, NETWORK, KEY, heights(), attempts,
                                  show=lambda *_: None)

    def outcomes(self, rows, kind):
        """The counted outcomes; an attempt that could not be made is made
        again (and three in a row stop the campaign)."""
        return {(r['variant'], r['outcome']) for r in rows if r['type'] == kind and r['outcome'] != 'transport'}


class Outcomes(unittest.TestCase):
    def row(self, kind, variant, **observed):
        return {'type': kind, 'variant': variant, 'observed': observed}

    def test_each_outcome_follows_from_the_observations(self):
        cases = [
            (self.row('connection_flood_address', 'channel', held=2, held_open=2, extra_bytes=0,
                      extra_closed_after=0.01), 'refused'),
            (self.row('connection_flood_address', 'channel', held=2, held_open=2, extra_bytes=0,
                      extra_closed_after=None), 'admitted'),
            (self.row('connection_flood_address', 'channel', held=2, held_open=1, extra_bytes=0,
                      extra_closed_after=0.01), 'transport'),
            (self.row('connection_flood_total', 'channel', held=16, held_open=16, extra_bytes=4437,
                      extra_closed_after=0.2), 'admitted'),
            (self.row('status_connection_flood', 'page', attacker_connections=8, attacker_open=2,
                      honest_status=503), 'refused'),
            (self.row('status_connection_flood', 'page', attacker_connections=8, attacker_open=8,
                      honest_status=None), 'admitted'),
            (self.row('slow_client', 'channel_header', bytes=0, closed_after=10.01), 'refused'),
            (self.row('slow_client', 'page_request_line', bytes=0, closed_after=None), 'admitted'),
            (self.row('slow_client', 'page_request_line', bytes=0, closed_after=30.0), 'admitted'),
            (self.row('invalid_handshake', 'wrong_key', reply_bytes=0, closed_after=0.0), 'refused'),
            (self.row('invalid_handshake', 'wrong_key', reply_bytes=4437, closed_after=0.1), 'admitted'),
            (self.row('invalid_handshake', 'bad_magic', reply_bytes=0, closed_after=None), 'admitted'),
            (self.row('invalid_handshake', 'forged_finish', offer_bytes=4437, after_bytes=0, closed_after=0.0),
             'refused'),
            (self.row('invalid_handshake', 'forged_finish', offer_bytes=0, after_bytes=0, closed_after=0.0),
             'refused_other'),
            (self.row('invalid_handshake', 'forged_finish', offer_bytes=4437, after_bytes=60, closed_after=0.1),
             'admitted'),
            (self.row('header_flood', 'too_many_headers', status=431), 'refused'),
            (self.row('header_flood', 'too_many_headers', status=None), 'refused'),
            (self.row('header_flood', 'too_many_header_bytes', status=200), 'admitted'),
            (self.row('header_flood', 'too_many_header_bytes', error='ConnectionRefusedError'), 'transport'),
        ]
        for row, outcome in cases:
            self.assertEqual(endpoints.classify(row), outcome, row)

    def test_only_approved_types_exist_and_every_one_is_built(self):
        self.assertEqual({k for k, v in endpoints.REASONS.items() if v is None}, set())
        self.assertEqual(set(endpoints.CLIENT_TYPES), {'oversized_body', 'method_outside_allowlist', 'wrong_pin'})
        with self.assertRaises(endpoints.Failed):
            endpoints.built('teardrop')

    def test_the_attack_client_types(self):
        def row(kind, **seen):
            return {'type': kind, 'variant': kind, 'observed': seen}
        handshake = {'handshake': True, 'write_error': None, 'response_bytes': 0}
        cases = [
            (row('oversized_body', **handshake, response_status=None, ended='eof'), 'refused'),
            (row('oversized_body', **handshake, response_status=None, ended='connection reset'), 'refused'),
            (row('oversized_body', **handshake, response_status=413, ended='response'), 'refused'),
            (row('oversized_body', **handshake, response_status=200, ended='response'), 'admitted'),
            (row('oversized_body', **handshake, response_status=None, ended='timed out'), 'refused_other'),
            (row('oversized_body', **handshake, response_status=502, ended='response'), 'refused_other'),
            (row('oversized_body', handshake=False, error='connection refused'), 'transport'),
            (row('method_outside_allowlist', **handshake, response_status=400, ended='response'), 'refused'),
            (row('method_outside_allowlist', **handshake, response_status=200, ended='response'), 'admitted'),
            (row('method_outside_allowlist', **handshake, response_status=None, ended='eof'), 'refused_other'),
            (row('wrong_pin', hello=True, offer_sent=True, after_offer_bytes=0, finished=False, cli_exit=1),
             'refused'),
            (row('wrong_pin', hello=True, offer_sent=True, after_offer_bytes=40, finished=True, cli_exit=1),
             'admitted'),
            (row('wrong_pin', hello=True, offer_sent=True, after_offer_bytes=0, finished=False, cli_exit=0),
             'admitted'),
            (row('wrong_pin', hello=False, error='no client connected', cli_exit=1), 'transport'),
        ]
        for one, outcome in cases:
            self.assertEqual(endpoints.classify(one), outcome, one)

    def test_the_bytes_of_each_invalid_handshake(self):
        good = endpoints.hello_payload(NETWORK, KEY)
        self.assertEqual(len(good), endpoints.HELLO_FIXED + len(NETWORK))
        self.assertTrue(key_valid(good[-endpoints.ENCAPSULATION_KEY:]))
        hello, _ = endpoints.handshake_bytes('invalid_encapsulation_key', NETWORK, KEY)
        self.assertFalse(key_valid(hello[-endpoints.ENCAPSULATION_KEY:]))
        hello, _ = endpoints.handshake_bytes('long_length', NETWORK, KEY)
        self.assertEqual(int.from_bytes(hello[6:8], 'big'), endpoints.HELLO_FIXED + 65)
        hello, finish = endpoints.handshake_bytes('forged_finish', NETWORK, KEY)
        self.assertEqual((hello[:6], finish[:8], len(finish)),
                         (b'DYCH\x01\x01', b'DYCH\x01\x03\x00\x20', 40))
        hello, _ = endpoints.handshake_bytes('wrong_network', NETWORK, KEY)
        self.assertEqual(hello[9:9 + len(NETWORK)], b'dytallix-staging-x')


def sample_rows(attempts=2, admitted=None, other=None):
    """Three types refused `attempts` times each, with a control after each;
    `admitted` serves one attempt of a type, `other` refuses one otherwise."""
    rows = [{'type': 'control', 'after': 'start', 'channel_height': 1, 'page_status': 200, 'page_height': 1}]
    seen = {
        'connection_flood_address': ('channel', {'held': 2, 'held_open': 2, 'extra_bytes': 0,
                                                 'extra_closed_after': 0.0}),
        'invalid_handshake': ('bad_magic', {'reply_bytes': 0, 'closed_after': 0.0}),
        'header_flood': ('too_many_headers', {'status': 431}),
    }
    for n, (kind, (variant, observed)) in enumerate(seen.items()):
        for k in range(attempts):
            row = {'type': kind, 'k': k, 'variant': variant, 'observed': dict(observed)}
            if kind == admitted and k == 0:
                row['observed']['extra_closed_after'] = None
            if kind == other and k == 0:
                row['variant'], row['observed'] = 'forged_finish', {'offer_bytes': 0, 'after_bytes': 0,
                                                                    'closed_after': 0.0}
            row['outcome'], row['log'] = endpoints.classify(row), endpoints.describe(row)
            rows.append(row)
        rows.append({'type': 'control', 'after': kind, 'channel_height': n + 2, 'page_status': 200,
                     'page_height': n + 2})
    return rows


class Records(unittest.TestCase):
    def rows(self, attempts=2, admitted=None, other=None):
        return sample_rows(attempts, admitted, other)

    def record(self, rows, attempts=2):
        return endpoints.build_record(NETWORK, GENESIS, ENGINE, rows, attempts, 's', 'f')

    def test_a_campaign_record_verifies_and_any_edit_shows(self):
        rows = self.rows()
        record = self.record(rows)
        self.assertEqual(record['status'], endpoints.PASS, record['checks'])
        self.assertEqual({k: t['status'] for k, t in record['types'].items()}, {
            'connection_flood_address': 'PASS', 'connection_flood_total': 'not_run',
            'status_connection_flood': 'not_run', 'slow_client': 'not_run', 'invalid_handshake': 'PASS',
            'header_flood': 'PASS', 'oversized_body': 'not_run', 'method_outside_allowlist': 'not_run',
            'wrong_pin': 'not_run'})
        raw = endpoints.rows_bytes(rows)
        self.assertEqual(endpoints.verify(record, raw, GENESIS, ENGINE), endpoints.PASS)
        edited = raw.replace(b'"extra_closed_after":0.0', b'"extra_closed_after":null', 1)
        with self.assertRaisesRegex(endpoints.Failed, 'rows are not'):
            endpoints.verify(record, edited, GENESIS, ENGINE)
        with self.assertRaisesRegex(endpoints.Failed, 'another genesis'):
            endpoints.verify(record, raw, b'{}', ENGINE)
        with mock.patch.dict(endpoints.LIMITS, {'channel_per_address': 4}):
            with self.assertRaisesRegex(endpoints.Failed, 'approved bounds'):
                endpoints.verify(record, raw, GENESIS, ENGINE)
        lying = dict(record, successes=0)
        lying['types'] = dict(record['types'], header_flood=dict(record['types']['header_flood'], status='FAIL'))
        with self.assertRaises(endpoints.Failed):
            endpoints.verify(lying, raw, GENESIS, ENGINE)

    def test_a_served_attempt_is_a_success_and_another_refusal_fails_the_type(self):
        record = self.record(self.rows(admitted='connection_flood_address'))
        self.assertEqual((record['status'], record['successes'],
                          record['types']['connection_flood_address']['status']), ('FAIL', 1, 'FAIL'))
        record = self.record(self.rows(other='invalid_handshake'))
        self.assertEqual((record['status'], record['successes'],
                          record['types']['invalid_handshake']['counts']['refused_other']), ('FAIL', 0, 1))

    def test_a_row_whose_outcome_does_not_follow_is_refused(self):
        rows = self.rows()
        rows[1]['outcome'] = 'admitted'
        with self.assertRaisesRegex(endpoints.Failed, 'does not follow'):
            self.record(rows)

    def test_the_controls_must_answer_and_the_chain_must_advance(self):
        rows = self.rows()
        rows[-1]['channel_height'] = rows[0]['channel_height']
        self.assertEqual((self.record(rows)['checks']['advancing'], self.record(rows)['status']), (False, 'FAIL'))
        rows = self.rows()
        rows[-1]['page_height'] = None
        self.assertEqual(self.record(rows)['checks']['controls'], False)

    def test_a_control_waits_for_the_next_block_but_not_for_a_stopped_chain(self):
        def campaign(heights):
            readings = iter(heights)
            control = lambda: {'channel_height': next(readings), 'page_status': 200, 'page_height': 5}
            return endpoints.Campaign(FakePlaces(), None, None, NETWORK, KEY, control, 2, show=lambda *_: None,
                                      sleep=lambda _: None)
        # A type quicker than a block: the same height twice, then the next.
        quick = campaign([10, 10, 10, 11])
        quick.positive('start')
        quick.positive('invalid_handshake')
        self.assertEqual([r['channel_height'] for r in quick.rows], [10, 11])
        # A chain that stopped is read until the wait ends, and does not advance.
        stopped = campaign(itertools.repeat(10))
        stopped.positive('start')
        ticks = itertools.count(0, 10)
        stopped.positive('header_flood', clock=lambda: next(ticks))
        self.assertEqual([r['channel_height'] for r in stopped.rows], [10, 10])
        self.assertLess(next(ticks), 10 * (endpoints.ADVANCE_WAIT / 10 + 4))

    def test_the_results_collector_reads_the_family(self):
        import results
        self.assertIs(results.ATTACK_FAMILIES['endpoint_rpc'], endpoints)


class Probes(Fast):
    def test_an_extra_connection_from_a_full_address_is_closed_at_once(self):
        channel = self.listen(FakeChannel(per_address=2))
        page = self.listen(FakePage())
        campaign = self.campaign(channel, page)
        campaign.until('connection_flood_address', campaign.flood_address)
        self.assertEqual(self.outcomes(campaign.rows, 'connection_flood_address'), {('channel', 'refused')})
        loose = self.listen(FakeChannel(per_address=3, deadline=5))
        campaign = self.campaign(loose, page)
        campaign.until('connection_flood_address', campaign.flood_address)
        self.assertEqual(self.outcomes(campaign.rows, 'connection_flood_address'), {('channel', 'admitted')})

    def test_invalid_handshakes_are_closed_with_no_reply(self):
        page = self.listen(FakePage())
        campaign = self.campaign(self.listen(FakeChannel()), page, attempts=len(endpoints.HANDSHAKES))
        campaign.until('invalid_handshake', campaign.handshake)
        self.assertEqual(self.outcomes(campaign.rows, 'invalid_handshake'),
                         {(v, 'refused') for v in endpoints.HANDSHAKES})
        campaign = self.campaign(self.listen(FakeChannel(lax=True)), page, attempts=len(endpoints.HANDSHAKES))
        campaign.until('invalid_handshake', campaign.handshake)
        served = {'wrong_network', 'wrong_key', 'invalid_encapsulation_key'}
        self.assertEqual(self.outcomes(campaign.rows, 'invalid_handshake'),
                         {(v, 'admitted' if v in served else 'refused') for v in endpoints.HANDSHAKES})
        # A listener that answers no hello cannot show a forged finish refused.
        campaign = self.campaign(self.listen(FakeChannel(mute=True)), page, attempts=len(endpoints.HANDSHAKES))
        campaign.until('invalid_handshake', campaign.handshake)
        self.assertIn(('forged_finish', 'refused_other'), self.outcomes(campaign.rows, 'invalid_handshake'))

    def test_stalled_connections_are_closed_within_the_deadline(self):
        campaign = self.campaign(self.listen(FakeChannel()), self.listen(FakePage()), attempts=6)
        campaign.until('slow_client', campaign.slow)
        self.assertEqual(self.outcomes(campaign.rows, 'slow_client'), {(v, 'refused') for v in endpoints.SLOW})
        campaign = self.campaign(self.listen(FakeChannel(deadline=3600)), self.listen(FakePage(deadline=3600)),
                                 attempts=6)
        campaign.until('slow_client', campaign.slow)
        self.assertEqual(self.outcomes(campaign.rows, 'slow_client'), {(v, 'admitted') for v in endpoints.SLOW})

    def test_requests_over_the_header_bounds_are_not_served(self):
        channel = self.listen(FakeChannel())
        campaign = self.campaign(channel, self.listen(FakePage()))
        campaign.until('header_flood', campaign.header_flood)
        self.assertEqual(self.outcomes(campaign.rows, 'header_flood'),
                         {(v, 'refused') for v in endpoints.HEADER_FLOODS})
        campaign = self.campaign(channel, self.listen(FakePage(lax=True)))
        campaign.until('header_flood', campaign.header_flood)
        self.assertEqual(self.outcomes(campaign.rows, 'header_flood'),
                         {(v, 'admitted') for v in endpoints.HEADER_FLOODS})

    def test_the_probe_runs_in_a_process_of_its_own(self):
        host, port = self.listen(FakeChannel())
        seen = endpoints.Places().run(None, {'step': 'cap', 'host': host, 'port': port, 'held': 2, 'wait': 0.5})
        self.assertEqual((seen['held_open'], seen['extra_bytes']), (2, 0))
        self.assertIsNotNone(seen['extra_closed_after'])

    def test_a_held_process_reports_when_released(self):
        host, port = self.listen(FakeChannel(deadline=30))
        places = endpoints.Places()
        process = places.hold(None, {'step': 'hold', 'host': host, 'port': port, 'count': 2, 'seconds': 30})
        self.assertEqual(places.ready(process), 2)
        self.assertEqual(places.release(process), {'held': 2, 'open': 2})

    def test_a_listener_that_cannot_be_reached_is_made_again_then_stops(self):
        server = socket.socket()
        server.bind(('127.0.0.1', 0))
        port = server.getsockname()[1]
        server.close()
        campaign = self.campaign(('127.0.0.1', port), self.listen(FakePage()))
        with self.assertRaisesRegex(endpoints.Failed, 'could not be made'):
            campaign.until('connection_flood_address', campaign.flood_address)
        self.assertEqual({r['outcome'] for r in campaign.rows}, {'transport'})


@unittest.skipUnless(LINUX, 'several loopback addresses need Linux')
class Addresses(Fast):
    def test_a_full_channel_closes_one_more_address_at_once(self):
        with mock.patch.dict(endpoints.LIMITS, {'channel_total': 4}):
            places = FakePlaces(spaces(endpoints.needed_for_total()))
            page = self.listen(FakePage())
            campaign = self.campaign(self.listen(FakeChannel(total=4, deadline=5)), page, places)
            campaign.until('connection_flood_total', campaign.flood_total)
            self.assertEqual(self.outcomes(campaign.rows, 'connection_flood_total'), {('channel', 'refused')})
            campaign = self.campaign(self.listen(FakeChannel(total=64, deadline=5)), page, places)
            campaign.until('connection_flood_total', campaign.flood_total)
            self.assertEqual(self.outcomes(campaign.rows, 'connection_flood_total'), {('channel', 'admitted')})

    def test_one_address_cannot_keep_the_status_page_from_another(self):
        places = FakePlaces(spaces(1))
        channel = self.listen(FakeChannel())
        campaign = self.campaign(channel, self.listen(FakePage(per_address=2)), places)
        campaign.until('status_connection_flood', campaign.status_flood)
        self.assertEqual(self.outcomes(campaign.rows, 'status_connection_flood'), {('page', 'refused')})
        # Without the per-address bound one address fills the page.
        campaign = self.campaign(channel, self.listen(FakePage(per_address=None)), places)
        campaign.until('status_connection_flood', campaign.status_flood)
        self.assertEqual(self.outcomes(campaign.rows, 'status_connection_flood'), {('page', 'admitted')})

    def test_a_whole_campaign_passes_and_verifies(self):
        with mock.patch.dict(endpoints.LIMITS, {'channel_total': 4}):
            places = FakePlaces(spaces(endpoints.needed_for_total()))
            campaign = self.campaign(self.listen(FakeChannel(total=4)), self.listen(FakePage()), places)
            rows = campaign.run([k for k in endpoints.REASONS if endpoints.REASONS[k]])
            record = endpoints.build_record(NETWORK, GENESIS, ENGINE, rows, 2, 's', 'f')
            self.assertEqual(record['status'], endpoints.PASS, (record['checks'], record['types']))
            # The attack client's types need --attack-client, which this campaign has not.
            self.assertEqual({t['status'] for k, t in record['types'].items()
                              if endpoints.REASONS[k] and k not in endpoints.CLIENT_TYPES}, {'PASS'})
            self.assertEqual({record['types'][k]['status'] for k in endpoints.CLIENT_TYPES}, {'not_run'})
            self.assertEqual(endpoints.verify(record, endpoints.rows_bytes(rows), GENESIS, ENGINE), endpoints.PASS)


if __name__ == '__main__':
    unittest.main()
