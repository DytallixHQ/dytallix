#!/usr/bin/env python3
"""The attack campaign's endpoint and RPC family (simulation plan,
statistical campaign; launch/approvals/P01_T06_SIMULATION_INPUTS_2026-10-14.json
and P01_T04_ATTACK_DECISIONS_2026-11-04.json): each type about 30 times
against the sentry's two public listeners, the client channel and the status
page, with zero successes.

  endpoints.py run --pin PIN.json --page URL --cli DYTALLIX --cli-home DIR
                   --out DIR --genesis NATIVE_GENESIS
                   --engine-genesis ENGINE_GENESIS [--attempts N]
                   [--types T,...] [--spaces NS,...]
  endpoints.py probe                (one step from a JSON plan on stdin)
  endpoints.py verify --evidence FILE --rows FILE --genesis NATIVE_GENESIS
                      --engine-genesis ENGINE_GENESIS

Each attacker step runs from an address of its own: `--spaces` names network
namespaces, each with one address, and `run` starts `probe` inside them (with
none, every step runs from the runner's own address and the types that need
several addresses are not run). Each type and what counts as the endpoint
refusing it:
  - connection_flood_address: one address holds the channel's per-address
    bound of idle connections and opens one more; the extra one is closed
    at once with no byte sent, while the held ones stay open;
  - connection_flood_total: enough addresses hold the channel's whole bound,
    and one more address opens a connection; it is closed at once;
  - status_connection_flood: one address opens the status page's whole bound
    of idle connections, and an honest reader at another address still gets
    the page;
  - slow_client: a connection sends part of a hello (or of the page's
    request line) and then nothing; the listener closes it within its own
    deadline;
  - invalid_handshake: a hello the endpoint must refuse (a bad magic,
    version, type or length; another network; another endpoint's key; an
    encapsulation key that fails the FIPS 203 check), or a forged finish
    after the endpoint's real offer; it is closed with no reply;
  - header_flood: a status page request over its header bounds (16 headers,
    8 KiB) is answered 400 or 431, or closed, never served.
  - oversized_body: the attack client completes the handshake and sends a
    POST whose body is one byte over the channel's 1 MiB; the endpoint
    answers 400 (the request codec refuses it) or 413 (over the adapter's
    lowered limit), or closes with no reply, and never serves it;
  - method_outside_allowlist: the attack client completes the handshake and
    sends a request whose method byte is neither GET nor POST; the endpoint
    answers 400, never serves it;
  - wrong_pin: the attack client listens as an endpoint that signs with a
    fresh key, and the release CLI, pinned to the real key at that address,
    asks it for the status; the CLI must fail and send nothing after the
    endpoint's offer.
These three need --attack-client (node/tools/attack-client, test-only, never
shipped); without it they count zero attempts, and the family's attempts
(the fewest over every type) cannot reach the target.

An attempt the listener served is a success (a defect); one refused in
another way than the type names (a forged finish whose valid hello was never
answered) is counted apart and fails the type; one that could not be made (a
connection that failed, a held connection the listener closed before the
extra one) is counted apart and made again. After each type a positive
control from the runner's own address must read the chain's height through
the channel and the status page, and the heights must rise from control to
control, so a listener that refuses everything, or a chain that stopped,
cannot pass.

verify recomputes every outcome, count, status and digest from the rows. The
rows and the evidence name no address; the evidence binds the genesis files.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import select
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import attacks  # noqa: E402
import reconcile  # noqa: E402

SCHEMA = attacks.SCHEMA
FAMILY = 'endpoint_rpc'
PASS, FAIL = attacks.PASS, attacks.FAIL
TARGET = 30
OUTCOMES = attacks.OUTCOMES
# Each approved endpoint and RPC type (launch-rehearsals.md, Statistical
# campaign) and what the listener must do; None: not built yet.
REASONS = {
    'connection_flood_address': 'an extra connection from a full address is closed at once',
    'connection_flood_total': 'an extra connection to a full channel is closed at once',
    'status_connection_flood': 'the status page still answers another address',
    'slow_client': 'a stalled connection is closed within the listener\'s deadline',
    'invalid_handshake': 'an invalid handshake is closed with no reply',
    'header_flood': 'a request over the header bounds is not served',
    'oversized_body': 'a body over the bound is answered 400 or 413, or closed with no reply',
    'method_outside_allowlist': 'a method outside GET and POST is answered 400, never served',
    'wrong_pin': 'the release client refuses an endpoint that signs with another key',
}
# The attack client's types (node/tools/attack-client; P01, 4 November 2026).
CLIENT_TYPES = ('oversized_body', 'method_outside_allowlist', 'wrong_pin')
# The approved bounds (launch/E05_VALUES.json, D12-Q01: the channel's 16 and
# 2; the adapter's compiled deadlines; the status page's 8 and 2, P01, 4
# November 2026).
LIMITS = {'channel_total': 16, 'channel_per_address': 2, 'channel_deadline': 10,
          'page_total': 8, 'page_per_address': 2, 'page_deadline': 2,
          'page_headers': 16, 'page_header_bytes': 8192}
# A refusal at once is seen within this; a stalled connection may take its
# deadline and this much more.
AT_ONCE = 3.0
SLACK = 5.0
# A listener frees a closed connection's slot a moment after the close; the
# next flood waits this long, so it meets the bound and not the last flood.
SETTLE = 0.5
# Blocks come about every 5 seconds, and some types make their attempts in
# less; the control after a type waits up to this long for a block newer
# than the last control's, so only a chain that stopped fails to advance.
ADVANCE_WAIT = 30.0
# The client channel's handshake (client-channel-v1.md; handshake.rs).
MAGIC, VERSION = b'DYCH', 1
HELLO, OFFER, FINISH = 1, 2, 3
NONCE, PUBLIC_KEY, ENCAPSULATION_KEY = 32, 1952, 1184
OFFER_LEN = 1088 + 3309 + 32
HELLO_FIXED = 1 + NONCE + PUBLIC_KEY + ENCAPSULATION_KEY
HANDSHAKES = ('bad_magic', 'bad_version', 'finish_first', 'short_length', 'long_length',
              'network_length_mismatch', 'wrong_network', 'wrong_key', 'invalid_encapsulation_key',
              'forged_finish')
SLOW = ('channel_header', 'channel_hello', 'page_request_line')
HEADER_FLOODS = ('too_many_headers', 'too_many_header_bytes')


class Failed(Exception):
    pass


digest, canonical, rows_bytes = attacks.digest, attacks.canonical, attacks.rows_bytes


def built(kind):
    if kind not in REASONS:
        raise Failed(f'{kind!r} is not an approved endpoint and RPC attack type')
    return REASONS[kind] is not None


def answered(value):
    return isinstance(value, int) and not isinstance(value, bool)


def classify(row):
    """An attempt's outcome from its observations alone."""
    kind, seen = row['type'], row['observed']
    if seen.get('error'):
        return 'transport'
    if kind in ('connection_flood_address', 'connection_flood_total'):
        if seen['held_open'] < seen['held']:
            return 'transport'
        return 'refused' if seen['extra_closed_after'] is not None and not seen['extra_bytes'] else 'admitted'
    if kind == 'status_connection_flood':
        return 'refused' if answered(seen['honest_status']) else 'admitted'
    if kind == 'slow_client':
        limit = LIMITS['page_deadline' if row['variant'].startswith('page') else 'channel_deadline'] + SLACK
        closed = seen['closed_after']
        return 'refused' if closed is not None and closed <= limit else 'admitted'
    if kind == 'invalid_handshake':
        if row['variant'] == 'forged_finish':
            if seen['offer_bytes'] != 8 + OFFER_LEN:
                return 'refused_other'
            return 'refused' if not seen['after_bytes'] and seen['closed_after'] is not None else 'admitted'
        return 'refused' if not seen['reply_bytes'] and seen['closed_after'] is not None else 'admitted'
    if kind == 'header_flood':
        return 'refused' if seen['status'] in (None, 400, 431) else 'admitted'
    if kind in ('oversized_body', 'method_outside_allowlist'):
        if not seen.get('handshake'):
            return 'transport'
        status = seen['response_status']
        if isinstance(status, int) and 200 <= status < 300:
            return 'admitted'
        if status is None:
            # Closed with no reply; a connection merely left open is not a refusal.
            held = any(word in (seen.get('ended') or '') for word in ('timed out', 'would block'))
            return 'refused_other' if held or kind == 'method_outside_allowlist' else 'refused'
        # A body over the channel's 1 MiB is refused by the request codec (400,
        # Malformed channel request; the message bound leaves room for the
        # longest path and query, so the message still arrives whole); one
        # over the adapter's lowered body limit is answered 413.
        allowed = (400, 413) if kind == 'oversized_body' else (400,)
        return 'refused' if status in allowed else 'refused_other'
    if kind == 'wrong_pin':
        if not seen.get('hello') or not seen.get('offer_sent'):
            return 'transport'
        if seen['finished'] or seen['after_offer_bytes'] or seen['cli_exit'] == 0:
            return 'admitted'
        return 'refused'
    raise Failed(f'{kind!r} has no observations to judge')


def describe(row):
    return f'{row["type"]}/{row["variant"]}: {json.dumps(row["observed"], sort_keys=True)}'


# The bytes each attempt sends

def frame(kind, payload, magic=MAGIC, version=VERSION, length=None):
    length = len(payload) if length is None else length
    return magic + bytes([version, kind]) + length.to_bytes(2, 'big') + payload


def hello_payload(network, key, encapsulation_key=bytes(ENCAPSULATION_KEY)):
    """A hello's payload. An all-zero encapsulation key passes the FIPS 203
    check (every coefficient below q), so the endpoint answers it."""
    network = network.encode()
    return bytes([len(network)]) + network + os.urandom(NONCE) + key + encapsulation_key


def handshake_bytes(variant, network, key):
    """(hello, finish): what one invalid_handshake attempt sends."""
    good = hello_payload(network, key)
    if variant == 'bad_magic':
        return frame(HELLO, good, magic=b'DYCX'), None
    if variant == 'bad_version':
        return frame(HELLO, good, version=VERSION + 1), None
    if variant == 'finish_first':
        return frame(FINISH, os.urandom(32)), None
    if variant == 'short_length':
        return frame(HELLO, good[:HELLO_FIXED], length=HELLO_FIXED), None
    if variant == 'long_length':
        return frame(HELLO, b'', length=HELLO_FIXED + 65), None
    if variant == 'network_length_mismatch':
        return frame(HELLO, bytes([good[0] + 1]) + good[1:]), None
    if variant == 'wrong_network':
        return frame(HELLO, hello_payload(network[:-1] + ('x' if network[-1] != 'x' else 'y'), key)), None
    if variant == 'wrong_key':
        other = bytes(b ^ 0xff for b in key[:32]) + key[32:]
        return frame(HELLO, hello_payload(network, other)), None
    if variant == 'invalid_encapsulation_key':
        return frame(HELLO, hello_payload(network, key, b'\xff' * ENCAPSULATION_KEY)), None
    if variant == 'forged_finish':
        return frame(HELLO, good), frame(FINISH, os.urandom(32))
    raise Failed(f'{variant!r} is not an invalid handshake')


def header_flood_bytes(variant, host):
    if variant == 'too_many_headers':
        extra = ''.join(f'X-Flood-{n}: {n}\r\n' for n in range(LIMITS['page_headers'] * 4))
    elif variant == 'too_many_header_bytes':
        extra = f'X-Flood: {"a" * (LIMITS["page_header_bytes"] * 2)}\r\n'
    else:
        raise Failed(f'{variant!r} is not a header flood')
    return f'GET /status HTTP/1.1\r\nHost: {host}\r\n{extra}Connection: close\r\n\r\n'.encode()


# Probing: plain sockets, from whichever address the process has

def connect(plan, timeout=5.0):
    """A connection from the process's address, or from `source` (tests
    give each place a loopback address of its own)."""
    source = (plan['source'], 0) if plan.get('source') else None
    sock = socket.create_connection((plan['host'], plan['port']), timeout=timeout, source_address=source)
    sock.setblocking(False)
    return sock


def is_open(sock):
    try:
        return sock.recv(1, socket.MSG_PEEK) != b''
    except BlockingIOError:
        return True
    except OSError:
        return False


def drain(sock, limit, clock=time.monotonic):
    """Reads until the peer closes or `limit` seconds pass: (bytes read,
    seconds to the close or None)."""
    start, total = clock(), 0
    while True:
        left = limit - (clock() - start)
        if left <= 0:
            return total, None
        ready, _, _ = select.select([sock], [], [], left)
        if not ready:
            continue
        try:
            data = sock.recv(65536)
        except BlockingIOError:
            continue
        except OSError:
            return total, round(clock() - start, 3)
        if not data:
            return total, round(clock() - start, 3)
        total += len(data)


def read_exactly(sock, count, limit, clock=time.monotonic):
    start, data = clock(), b''
    while len(data) < count:
        left = limit - (clock() - start)
        if left <= 0:
            break
        ready, _, _ = select.select([sock], [], [], left)
        if not ready:
            continue
        try:
            chunk = sock.recv(count - len(data))
        except BlockingIOError:
            continue
        except OSError:
            break
        if not chunk:
            break
        data += chunk
    return data


def send(sock, data):
    """Sends what the listener takes. A listener that closes before taking it
    all has refused it, so a reset here is an observation, not an error."""
    sock.setblocking(True)
    try:
        sock.sendall(data)
        return None
    except OSError as error:
        return type(error).__name__
    finally:
        sock.setblocking(False)


def probe_cap(plan):
    """Holds `held` idle connections, then opens one more."""
    held = [connect(plan) for _ in range(plan['held'])]
    time.sleep(0.2)
    try:
        extra = connect(plan)
        extra_bytes, closed = drain(extra, plan['wait'])
        extra.close()
        return {'held': plan['held'], 'held_open': sum(map(is_open, held)), 'extra_bytes': extra_bytes,
                'extra_closed_after': closed}
    finally:
        for sock in held:
            sock.close()


def probe_hold(plan, lines=sys.stdin, out=sys.stdout):
    """Holds `count` idle connections until stdin closes (or `seconds`), and
    reports how many were still open then."""
    held = [connect(plan) for _ in range(plan['count'])]
    out.write(json.dumps({'ready': len(held)}) + '\n')
    out.flush()
    select.select([lines], [], [], plan.get('seconds', 8))
    report = {'held': len(held), 'open': sum(map(is_open, held))}
    for sock in held:
        sock.close()
    return report


def probe_slow(plan):
    """`count` connections that each send `data` and then stall."""
    socks = [connect(plan) for _ in range(plan['count'])]
    try:
        for sock in socks:
            send(sock, bytes.fromhex(plan['data']))
        start, closed, sizes = time.monotonic(), [None] * len(socks), [0] * len(socks)
        while time.monotonic() - start < plan['limit'] and None in closed:
            ready, _, _ = select.select([s for s, c in zip(socks, closed) if c is None], [], [], 0.5)
            for sock in ready:
                n = socks.index(sock)
                try:
                    data = sock.recv(65536)
                except BlockingIOError:
                    continue
                except OSError:
                    data = b''
                if data:
                    sizes[n] += len(data)
                else:
                    closed[n] = round(time.monotonic() - start, 3)
        return [{'bytes': b, 'closed_after': c} for b, c in zip(sizes, closed)]
    finally:
        for sock in socks:
            sock.close()


def probe_handshake(plan):
    sock = connect(plan)
    try:
        cut = send(sock, bytes.fromhex(plan['hello']))
        if not plan.get('finish') or cut:
            reply, closed = drain(sock, plan['wait'])
            return {'reply_bytes': reply, 'closed_after': closed}
        offer = read_exactly(sock, 8 + OFFER_LEN, plan['wait'])
        if len(offer) == 8 + OFFER_LEN:
            send(sock, bytes.fromhex(plan['finish']))
        after, closed = drain(sock, plan['wait'])
        return {'offer_bytes': len(offer), 'after_bytes': after, 'closed_after': closed}
    finally:
        sock.close()


def probe_http(plan):
    sock = connect(plan)
    try:
        send(sock, bytes.fromhex(plan['data']))
        start, raw = time.monotonic(), b''
        while time.monotonic() - start < plan['wait']:
            ready, _, _ = select.select([sock], [], [], 0.5)
            if not ready:
                continue
            try:
                chunk = sock.recv(65536)
            except BlockingIOError:
                continue
            except OSError:
                break
            if not chunk:
                break
            raw += chunk
        return {'status': http_status(raw)}
    finally:
        sock.close()


def http_status(raw):
    if raw[:5] != b'HTTP/' or len(raw) < 12:
        return None
    try:
        return int(raw[9:12])
    except ValueError:
        return None


def probe_attack(plan):
    """The attack client's channel-request, run from this address: its one
    JSON line."""
    result = subprocess.run(plan['argv'], capture_output=True, text=True, timeout=120)
    if result.returncode != 0:
        return {'handshake': False, 'error': result.stderr.strip()[-200:]}
    return json.loads(result.stdout.strip().splitlines()[-1])


PROBES = {'cap': probe_cap, 'hold': probe_hold, 'slow': probe_slow, 'handshake': probe_handshake,
          'http': probe_http, 'attack': probe_attack}


def probe(plan):
    try:
        return PROBES[plan['step']](plan)
    except OSError as error:
        return {'error': type(error).__name__}


# Where each step runs

class Places:
    """Runs probe steps from the runner's address (`None`) or inside the
    named network namespaces, one address each."""

    def __init__(self, spaces=()):
        self.spaces = list(spaces)

    def command(self, space):
        here = [sys.executable, '-I', '-B', str(Path(__file__).resolve()), 'probe']
        return here if space is None else ['ip', 'netns', 'exec', space, *here]

    def run(self, space, plan):
        result = subprocess.run(self.command(space), input=json.dumps(plan), capture_output=True, text=True,
                                timeout=120)
        if result.returncode != 0:
            raise Failed(f'the probe in {space or "the runner"} failed: {result.stderr.strip()[-300:]}')
        return json.loads(result.stdout.splitlines()[-1])

    def hold(self, space, plan):
        process = subprocess.Popen(self.command(space), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   text=True)
        process.stdin.write(json.dumps(plan) + '\n')
        process.stdin.flush()
        return process

    @staticmethod
    def ready(process, timeout=15):
        ready, _, _ = select.select([process.stdout], [], [], timeout)
        line = process.stdout.readline() if ready else ''
        return json.loads(line).get('ready') if line.strip() else None

    @staticmethod
    def release(process):
        # communicate() closes stdin itself, which ends the hold; closed here
        # first, its flush raises on Python 3.12 (the runner's).
        out, _ = process.communicate(timeout=30)
        return json.loads(out.splitlines()[-1]) if out.strip() else {'held': 0, 'open': 0}


# The campaign

class Campaign:
    def __init__(self, places, channel, page, network, key, control, attempts, show=print, sleep=time.sleep,
                 attack_client=None, pin=None, cli=None, env=None, scratch=None):
        self.attack_client, self.pin, self.cli, self.env = attack_client, pin, cli, env
        self.scratch = Path(scratch) if scratch else None
        self.places, self.channel, self.page = places, channel, page
        self.network, self.key, self.control = network, key, control
        self.attempts, self.show, self.sleep, self.rows = attempts, show, sleep, []
        attackers = places.spaces or [None]
        self.attacker = attackers[0]

    def record(self, kind, k, variant, observed):
        row = {'type': kind, 'k': k, 'variant': variant, 'observed': observed}
        row['outcome'] = classify(row)
        row['log'] = describe(row)
        self.rows.append(row)
        return row['outcome']

    def counted(self, kind):
        return sum(1 for r in self.rows if r['type'] == kind and r['outcome'] != 'transport')

    def until(self, kind, attempt, tries=3):
        """Makes attempts until `attempts` count; a transport outcome is made
        again, at most `tries` times in a row."""
        failures = 0
        while self.counted(kind) < self.attempts:
            k = self.counted(kind)
            if attempt(k) == 'transport':
                failures += 1
                if failures >= tries:
                    raise Failed(f'{kind}: {tries} attempts in a row could not be made: {self.rows[-1]["log"]}')
            else:
                failures = 0

    def flood_address(self, k):
        host, port = self.channel
        seen = self.places.run(self.attacker, {'step': 'cap', 'host': host, 'port': port,
                                               'held': LIMITS['channel_per_address'], 'wait': AT_ONCE})
        self.sleep(SETTLE)
        return self.record('connection_flood_address', k, 'channel', seen)

    def flood_total(self, k):
        host, port = self.channel
        per = LIMITS['channel_per_address']
        holders, extra = self.places.spaces[:-1], self.places.spaces[-1]
        processes = [self.places.hold(space, {'step': 'hold', 'host': host, 'port': port, 'count': per})
                     for space in holders]
        try:
            held = sum(self.places.ready(p) or 0 for p in processes)
            seen = self.places.run(extra, {'step': 'cap', 'host': host, 'port': port, 'held': 0, 'wait': AT_ONCE})
        finally:
            reports = [self.places.release(p) for p in processes]
        seen.update(held=len(holders) * per, held_open=min(held, sum(r['open'] for r in reports)))
        self.sleep(SETTLE)
        return self.record('connection_flood_total', k, 'channel', seen)

    def status_flood(self, k):
        host, port = self.page
        process = self.places.hold(self.attacker, {'step': 'hold', 'host': host, 'port': port,
                                                   'count': LIMITS['page_total']})
        try:
            self.places.ready(process)
            honest = self.places.run(None, {'step': 'http', 'host': host, 'port': port,
                                            'data': page_request(host).hex(), 'wait': AT_ONCE})
        finally:
            report = self.places.release(process)
        seen = {'attacker_connections': report['held'], 'attacker_open': report['open'],
                'honest_status': honest.get('status')}
        if honest.get('error'):
            seen['honest_error'] = honest['error']
        self.sleep(SETTLE)
        return self.record('status_connection_flood', k, 'page', seen)

    def slow(self, k):
        """Stalls the address's whole bound of connections at once (two
        attempts), each of one variant."""
        variant = SLOW[(k // 2) % len(SLOW)]
        if variant.startswith('page'):
            (host, port), data = self.page, b'GET /status HTTP/1.1\r\n'
            limit, count = LIMITS['page_deadline'] + SLACK + 1, LIMITS['page_per_address']
        else:
            (host, port), limit = self.channel, LIMITS['channel_deadline'] + SLACK + 1
            count = LIMITS['channel_per_address']
            hello = frame(HELLO, hello_payload(self.network, self.key))
            data = hello[:7] if variant == 'channel_header' else hello[:8 + 40]
        seen = self.places.run(self.attacker, {'step': 'slow', 'host': host, 'port': port, 'count': count,
                                               'data': data.hex(), 'limit': limit})
        self.sleep(SETTLE)
        outcomes = [self.record('slow_client', k + n, variant, one)
                    for n, one in enumerate(seen if isinstance(seen, list) else [seen])]
        return 'transport' if 'transport' in outcomes else outcomes[-1]

    def handshake(self, k):
        variant = HANDSHAKES[k % len(HANDSHAKES)]
        host, port = self.channel
        hello, finish = handshake_bytes(variant, self.network, self.key)
        seen = self.places.run(self.attacker, {'step': 'handshake', 'host': host, 'port': port, 'hello': hello.hex(),
                                               'finish': finish.hex() if finish else None,
                                               'wait': AT_ONCE + LIMITS['channel_deadline']})
        return self.record('invalid_handshake', k, variant, seen)

    def header_flood(self, k):
        variant = HEADER_FLOODS[k % len(HEADER_FLOODS)]
        host, port = self.page
        seen = self.places.run(self.attacker, {'step': 'http', 'host': host, 'port': port,
                                               'data': header_flood_bytes(variant, host).hex(),
                                               'wait': AT_ONCE + LIMITS['page_deadline']})
        return self.record('header_flood', k, variant, seen)

    def client_request(self, kind):
        def attempt(k):
            seen = self.places.run(self.attacker, {'step': 'attack', 'argv': [
                str(self.attack_client), 'channel-request', '--pin', str(self.pin), '--variant', kind,
                '--wait', str(AT_ONCE + LIMITS['channel_deadline'])]})
            self.sleep(SETTLE)
            return self.record(kind, k, kind, seen)
        return attempt

    def wrong_pin(self, k):
        """The release CLI, pinned to the real key at the attack endpoint's
        address, asks it for the status."""
        with socket.socket() as probe_socket:
            probe_socket.bind(('127.0.0.1', 0))
            port = probe_socket.getsockname()[1]
        endpoint = subprocess.Popen([str(self.attack_client), 'wrong-key-endpoint', '--listen', f'127.0.0.1:{port}',
                                     '--network', self.network, '--wait', str(AT_ONCE + LIMITS['channel_deadline'])],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            ready = endpoint.stdout.readline()
            if not ready.strip() or not json.loads(ready).get('ready'):
                endpoint.kill()
                return self.record('wrong_pin', k, 'other_key', {'hello': False, 'error': 'not ready'})
            home = self.scratch / f'wrong-pin-{k}'
            (home / '.dytallix').mkdir(parents=True, exist_ok=True)
            chain = json.loads((Path(self.env['HOME']) / '.dytallix' / 'chain.json').read_bytes())
            chain['endpoint'] = f'127.0.0.1:{port}'
            (home / '.dytallix' / 'chain.json').write_text(json.dumps(chain))
            cli = subprocess.run([str(self.cli), 'control', 'status', '--out', str(home / 'status.json')],
                                 capture_output=True, text=True, env=dict(self.env, HOME=str(home)), timeout=60)
            out, _ = endpoint.communicate(timeout=60)
        finally:
            if endpoint.poll() is None:
                endpoint.kill()
        try:
            seen = json.loads(out.strip().splitlines()[-1])
        except (ValueError, IndexError):
            seen = {'hello': False, 'error': 'no report'}
        seen['cli_exit'] = cli.returncode
        return self.record('wrong_pin', k, 'other_key', seen)

    def positive(self, after, clock=time.monotonic):
        last = next((r.get('channel_height') for r in reversed(self.rows) if r['type'] == 'control'), None)
        reading, deadline = self.control(), clock() + ADVANCE_WAIT
        while (answered(last) and answered(reading.get('channel_height')) and reading['channel_height'] <= last
               and clock() < deadline):
            self.sleep(1)
            reading = self.control()
        row = {'type': 'control', 'after': after, **reading}
        self.rows.append(row)
        return row

    def run(self, kinds):
        attempt = {'connection_flood_address': self.flood_address, 'connection_flood_total': self.flood_total,
                   'status_connection_flood': self.status_flood, 'slow_client': self.slow,
                   'invalid_handshake': self.handshake, 'header_flood': self.header_flood,
                   'oversized_body': self.client_request('oversized_body'),
                   'method_outside_allowlist': self.client_request('method_outside_allowlist'),
                   'wrong_pin': self.wrong_pin}
        self.positive('start')
        for kind in kinds:
            if not built(kind):
                continue
            if kind == 'connection_flood_total' and len(self.places.spaces) < needed_for_total():
                self.show(f'== {kind}: needs {needed_for_total()} addresses; not run')
                continue
            if kind in CLIENT_TYPES and not self.attack_client:
                self.show(f'== {kind}: needs --attack-client; not run')
                continue
            if kind == 'status_connection_flood' and not self.places.spaces:
                self.show(f'== {kind}: needs an address besides the runner\'s; not run')
                continue
            self.show(f'== {kind}: {self.attempts} attempts')
            self.until(kind, attempt[kind])
            self.positive(kind)
        return self.rows


def needed_for_total():
    per = LIMITS['channel_per_address']
    return -(-LIMITS['channel_total'] // per) + 1


def page_request(host):
    return f'GET /status HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n'.encode()


def tally(rows, attempts):
    """Each type's outcome counts, first log and status: PASS once the wanted
    attempts were refused with no success and no other refusal."""
    types = {}
    for kind in REASONS:
        mine = [r for r in rows if r['type'] == kind]
        counts = {o: sum(1 for r in mine if r['outcome'] == o) for o in OUTCOMES}
        if not built(kind):
            status = 'not_built'
        elif counts['admitted'] or counts['refused_other']:
            status = FAIL
        elif not mine:
            status = 'not_run'
        else:
            status = PASS if counts['refused'] >= attempts else FAIL
        types[kind] = {'reason': REASONS[kind], 'counts': counts, 'status': status,
                       'first_log': next((r['log'] for r in mine), None)}
    return types


def control_answered(row):
    return answered(row.get('channel_height')) and answered(row.get('page_height'))


def build_record(chain_id, genesis_bytes, engine_genesis_bytes, rows, attempts, started, finished):
    for row in rows:
        if row['type'] != 'control':
            if classify(row) != row['outcome'] or describe(row) != row['log']:
                raise Failed(f'a {row["type"]} row\'s outcome does not follow from its observations')
    raw = rows_bytes(rows)
    types = tally(rows, attempts)
    controls = [r for r in rows if r['type'] == 'control']
    heights = [r.get('channel_height') for r in controls]
    checks = {
        'types': all(t['status'] in (PASS, 'not_built', 'not_run') for t in types.values()),
        'controls': bool(controls) and all(map(control_answered, controls)),
        'advancing': all(map(answered, heights)) and all(a < b for a, b in zip(heights, heights[1:])),
    }
    body = {'schema': SCHEMA, 'family': FAMILY, 'chain_id': chain_id,
            'genesis': {'native_genesis_sha256': digest(genesis_bytes),
                        'engine_genesis_sha256': digest(engine_genesis_bytes)},
            'started': started, 'finished': finished, 'attempts_wanted': attempts, 'limits': dict(LIMITS),
            'types': types,
            'controls': {'made': len(controls), 'answered': sum(map(control_answered, controls))},
            'successes': sum(t['counts']['admitted'] for t in types.values()),
            'rows_sha256': digest(raw), 'checks': checks,
            'status': PASS if all(checks.values()) else FAIL}
    body['sha256'] = digest(canonical(body))
    return body


def verify(record, raw, genesis_bytes, engine_genesis_bytes):
    """Re-evaluates a campaign record from its rows."""
    body = {k: v for k, v in record.items() if k != 'sha256'}
    if record.get('schema') != SCHEMA or digest(canonical(body)) != record.get('sha256'):
        raise Failed('the evidence digest does not match its contents')
    if record['family'] != FAMILY:
        raise Failed(f'the record is for {record["family"]}, not {FAMILY}')
    if record['limits'] != LIMITS:
        raise Failed(f'the record was judged against {record["limits"]}, not the approved bounds')
    if digest(raw) != record['rows_sha256']:
        raise Failed('the rows are not the ones the evidence names')
    if record['genesis'] != {'native_genesis_sha256': digest(genesis_bytes),
                             'engine_genesis_sha256': digest(engine_genesis_bytes)}:
        raise Failed('the evidence names another genesis')
    rows = [json.loads(line) for line in raw.splitlines() if line.strip()]
    again = build_record(record['chain_id'], genesis_bytes, engine_genesis_bytes, rows, record['attempts_wanted'],
                         record['started'], record['finished'])
    if again != record:
        raise Failed('the counts and statuses do not follow from the rows')
    return record['status']


def read_pin(path):
    pin = json.loads(Path(path).read_bytes())
    host, _, port = pin['address'].rpartition(':')
    return pin['network'], (host.strip('[]'), int(port)), base64.b64decode(pin['public_key_base64'])


def honest_reader(client, page):
    """The positive control: the chain's height through the channel (the
    release CLI) and through the status page, from the runner's address."""
    def control():
        out = {}
        try:
            out['channel_height'] = int(client.status()['height'])
        except (reconcile.Failed, KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
            out['channel_error'] = str(error)[:200]
        out['page_status'], out['page_height'] = read_page(page)
        return out
    return control


def read_page(page):
    """The status page's HTTP status and height; 503 still carries it."""
    try:
        with urllib.request.urlopen(f'http://{page[0]}:{page[1]}/status', timeout=5) as response:
            code, raw = response.status, response.read(4096)
    except urllib.error.HTTPError as error:
        code, raw = error.code, error.read(4096)
    except OSError:
        return None, None
    try:
        return code, int(json.loads(raw)['height'])
    except (ValueError, KeyError, TypeError):
        return code, None


def stamp():
    return time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())


def run_campaign(pin, page_url, cli, env, out, attempts, kinds, spaces, genesis_bytes, engine_genesis_bytes,
                 show=print, attack_client=None):
    """`env` runs the release CLI pinned to the endpoint (its HOME)."""
    network, channel, key = read_pin(pin)
    parts = urllib.parse.urlsplit(page_url)
    page = (parts.hostname, parts.port or 80)
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    client = reconcile.Client(cli, env, out)
    started = stamp()
    campaign = Campaign(Places(spaces), channel, page, network, key, honest_reader(client, page), attempts, show,
                        attack_client=attack_client, pin=pin, cli=cli, env=env, scratch=out / 'scratch')
    rows = campaign.run(kinds)
    record = build_record(network, genesis_bytes, engine_genesis_bytes, rows, attempts, started, stamp())
    (out / 'endpoints.jsonl').write_bytes(rows_bytes(rows))
    (out / 'endpoints.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


def report(record, show=print):
    for kind, t in record['types'].items():
        show(f'== {kind}: {t["status"]} {t["counts"]}; first: {(t["first_log"] or "")[:160]!r}')
    show(f'== controls {record["controls"]}, successes {record["successes"]}, checks {record["checks"]}: '
         f'{record["status"]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    r = commands.add_parser('run')
    for name in ('--pin', '--cli', '--cli-home', '--out', '--genesis', '--engine-genesis'):
        r.add_argument(name, type=Path, required=True)
    r.add_argument('--page', required=True, help='the status page, http://HOST:PORT/status')
    r.add_argument('--attempts', type=int, default=TARGET)
    r.add_argument('--types', default=','.join(k for k in REASONS if REASONS[k]))
    r.add_argument('--spaces', default='', help='network namespaces, one address each')
    r.add_argument('--attack-client', type=Path, help="node/tools/attack-client's dytallix-attack")
    commands.add_parser('probe')
    v = commands.add_parser('verify')
    for name in ('--evidence', '--rows', '--genesis', '--engine-genesis'):
        v.add_argument(name, type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'probe':
            plan = json.loads(sys.stdin.readline())
            print(json.dumps(probe(plan)), flush=True)
            return 0
        if args.command == 'verify':
            status = verify(json.loads(args.evidence.read_bytes()), args.rows.read_bytes(),
                            args.genesis.read_bytes(), args.engine_genesis.read_bytes())
            print(status)
            return 0 if status == PASS else 1
        kinds = [k for k in args.types.split(',') if k]
        for kind in kinds:
            built(kind)
        record = run_campaign(args.pin, args.page, args.cli, dict(os.environ, HOME=str(args.cli_home)), args.out,
                              args.attempts, kinds,
                              [s for s in args.spaces.split(',') if s], args.genesis.read_bytes(),
                              args.engine_genesis.read_bytes(), attack_client=args.attack_client)
        report(record)
        return 0 if record['status'] == PASS else 1
    except (Failed, reconcile.Failed, OSError, KeyError, ValueError) as error:
        print(f'endpoints: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
