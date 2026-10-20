#!/usr/bin/env python3
"""The founder's side of the staging agent (simulation plan, T2, the
real-host harness; node/docs/architecture/staging-agent-v1.md). The
production hosts are console-only, so the founder's machine reaches them
only through three public paths, and this module uses nothing else:

- the command store's S3 API, signed with AWS Signature V4 by curl: PUT a
  signed command, LIST and GET the agent's sealed results, which are then
  opened with `dytallix-root-sign history-open` and the chain's history code;
- the sentry's public status page (`{"chain_id","height","time"}`);
- the release CLI pinned to the sentry's client channel (the other tools).

The CI drills drive the agent through this module too, so a drill that
passes there passes through the same calls on a real host.

  staging_founder.py send --store STORE.json --cli DYTALLIX --home DIR --key KEY.json
                          --chain ID --host LABEL --command NAME [--arg NAME=INT ...]
                          [--paper FILE --signer DYTALLIX_ROOT_SIGN --work DIR --wait S]
  staging_founder.py open --store STORE.json --chain ID --host LABEL --paper FILE
                          --signer DYTALLIX_ROOT_SIGN --work DIR
  staging_founder.py reconcile --store STORE.json --cli DYTALLIX --home DIR --key KEY.json --chain ID
                               --host LABEL [--host LABEL ...] --paper FILE --signer DYTALLIX_ROOT_SIGN
                               --work DIR --genesis NATIVE_GENESIS --out EVIDENCE
                               [--client-home DIR [--addresses FILE]] [--since FILE --ledger FILE]
  staging_founder.py status-page --url URL
  staging_founder.py preflight --store STORE.json --chain ID --host LABEL [--status-url URL]

STORE.json (`dytallix.staging-founder-store.v1`) holds the founder's own key
for the staging bucket, the command prefix and the results prefix (the hosts'
history upload prefix); it stays on the founder's machine. send signs one
command with the staging command key at the next free sequence (one past the
highest the store holds for that host), uploads it, and with --wait opens its
result. reconcile is the reconciliation report (T6) on console-only hosts:
reconcile-read sent to every host for the same minute, the client's status
view read meanwhile through the pinned CLI home given, and the readings the
sealed results carry evaluated (reconcile.collect_through_agent); it exits 0
only on PASS. preflight reads, changes nothing, and lists the checks that only the
founder can make on the real store and hosts.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
from urllib.parse import quote, urlsplit
import urllib.error
import urllib.request
import xml.etree.ElementTree as ElementTree

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import reconcile  # noqa: E402
import staging_command  # noqa: E402
agent = staging_command.agent

STORE_SCHEMA = 'dytallix.staging-founder-store.v1'
STORE_FIELDS = ('endpoint', 'bucket', 'region', 'access_key_id', 'secret_access_key',
                'commands_prefix', 'results_prefix')
RESULT_NAME = re.compile(r'^([0-9]{10})-([0-9a-f]{64})\.bin$')
COMMAND_NAME = re.compile(r'^([0-9]{10})\.json$')
S3 = '{http://s3.amazonaws.com/doc/2006-03-01/}'
MAX_RESULT_BYTES = 4 << 20
STATUS_FIELDS = ('chain_id', 'height', 'time')


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def iso(seconds):
    return datetime.fromtimestamp(seconds, tz=timezone.utc).isoformat()


# The store

def check_store(store, standin=False):
    """The founder's store record. TLS is only transport; plain HTTP is
    allowed only to a CI stand-in, and only when the caller says so."""
    require(type(store) is dict and store.get('schema') == STORE_SCHEMA, f'not a founder store record ({STORE_SCHEMA})')
    require(set(store) == {'schema', *STORE_FIELDS}, 'the founder store record has missing or extra fields')
    endpoint = urlsplit(store['endpoint'])
    require(endpoint.scheme == 'https' or (standin and endpoint.scheme == 'http'),
            'the store endpoint must use https (plain http only to the CI stand-in)')
    require(endpoint.path in ('', '/') and not endpoint.query and not endpoint.username, 'the endpoint is a bare origin')
    for field in ('bucket', 'region', 'access_key_id', 'secret_access_key'):
        require(type(store[field]) is str and agent.SAFE.match(store[field]), f'the store\'s {field} has unexpected characters')
    for field in ('commands_prefix', 'results_prefix'):
        require(type(store[field]) is str and agent.PREFIX.match(store[field]), f'the store\'s {field} is not a key prefix (segments, each ending in /)')
    return store


def load_store(path, standin=False):
    return check_store(json.loads(Path(path).read_bytes()), standin)


def object_url(store, key, query=''):
    return f'{store["endpoint"].rstrip("/")}/{store["bucket"]}/{quote(key, safe="/")}{query}'


class Store:
    """PUT, GET and LIST on the command store through curl, signed with the
    founder's key. curl's configuration (and so the key) goes on its standard
    input, never on a command line."""

    def __init__(self, store, scratch, curl='curl', run=subprocess.run):
        self.store, self.scratch, self.curl, self.run = store, Path(scratch), curl, run
        self.scratch.mkdir(parents=True, exist_ok=True)

    def call(self, lines, what):
        result = self.run([self.curl, '--config', '-'], input=agent.curl_config(self.store, lines),
                          capture_output=True, timeout=120)
        require(result.returncode == 0, f'{what}: curl exited {result.returncode}: '
                                        f'{(result.stderr or b"").decode(errors="replace").strip()}')
        return int((result.stdout or b'0').decode().strip() or 0)

    def get(self, key, limit=MAX_RESULT_BYTES):
        target = self.scratch / 'get.bin'
        target.unlink(missing_ok=True)
        status = self.call([f'max-filesize = {limit}', f'output = "{target}"', 'write-out = "%{http_code}"',
                            f'url = "{object_url(self.store, key)}"'], f'GET {key}')
        raw = target.read_bytes() if status == 200 and target.exists() else None
        target.unlink(missing_ok=True)
        return status, raw

    def put(self, key, raw):
        status, _ = self.get(key)
        require(status == 404, f'{key} already exists in the store (HTTP {status}); never replace a command')
        source = self.scratch / 'put.bin'
        source.write_bytes(raw)
        try:
            status = self.call([f'upload-file = "{source}"', f'output = "{self.scratch / "put.out"}"',
                                'write-out = "%{http_code}"', f'url = "{object_url(self.store, key)}"'], f'PUT {key}')
        finally:
            source.unlink(missing_ok=True)
            (self.scratch / 'put.out').unlink(missing_ok=True)
        require(status == 200, f'PUT {key} answered HTTP {status}')

    def list(self, prefix):
        """Every key under the prefix (ListObjectsV2, following continuations)."""
        keys, token = [], None
        while True:
            query = f'?list-type=2&prefix={quote(prefix, safe="")}'
            if token:
                query += f'&continuation-token={quote(token, safe="")}'
            target = self.scratch / 'list.xml'
            target.unlink(missing_ok=True)
            status = self.call([f'output = "{target}"', 'write-out = "%{http_code}"',
                                f'url = "{object_url(self.store, "", query)}"'], f'LIST {prefix}')
            raw = target.read_bytes() if target.exists() else b''
            target.unlink(missing_ok=True)
            require(status == 200, f'LIST {prefix} answered HTTP {status}: {raw[:200]!r}')
            root = ElementTree.fromstring(raw)
            keys += [node.text for node in root.iter(f'{S3}Key')]
            truncated = (root.findtext(f'{S3}IsTruncated') or 'false') == 'true'
            token = root.findtext(f'{S3}NextContinuationToken')
            if not truncated or not token:
                return keys


# The agent channel

class Channel:
    """Signed commands to one host's agent and its opened results."""

    def __init__(self, store, chain, host, scratch, cli=None, home=None, key_path=None, paper=None, signer=None,
                 clock=time.time, sleep=time.sleep, say=print):
        """Sending needs the CLI, its keystore home and the key record; opening
        needs the history code paper and dytallix-root-sign."""
        self.store, self.chain, self.host = store, chain, host
        self.cli, self.home = cli, home
        self.key = staging_command.load_key(key_path) if key_path else None
        self.scratch, self.paper, self.signer = Path(scratch), paper, signer
        self.clock, self.sleep, self.say = clock, sleep, say
        # Each opened result's object name and digest, which the evidence binds.
        self.sent, self.opened, self.names = {}, {}, {}
        self.scratch.mkdir(parents=True, exist_ok=True)

    @property
    def commands(self):
        return f'{self.store.store["commands_prefix"]}{self.chain}/{self.host}/'

    @property
    def results(self):
        return f'{self.store.store["results_prefix"]}{self.chain}/agent/{self.host}/'

    def next_sequence(self):
        """One past the highest command the store holds for this host, so a
        refused or planted object is never reused."""
        found = [int(m.group(1)) for k in self.store.list(self.commands)
                 if (m := COMMAND_NAME.match(k[len(self.commands):]))]
        return max(found, default=0) + 1

    def send(self, command, args=None, valid_seconds=3600):
        require(self.key and self.cli and self.home, 'sending needs the CLI, its home and the key record')
        args = dict(args or {})
        staging_command.check_allowed(command, args)
        require(60 <= valid_seconds <= agent.MAX_VALIDITY_SECONDS, 'a command is valid for 1 minute to 24 hours')
        sequence = self.next_sequence()
        unsigned = staging_command.body(self.chain, self.host, sequence, command, args,
                                        int(self.clock()) + valid_seconds)
        raw = staging_command.sign(self.cli, self.home, self.key, unsigned)
        self.store.put(f'{self.commands}{sequence:010}.json', raw)
        self.sent[sequence] = hashlib.sha256(raw).hexdigest()
        self.say(f'== {self.host}: sent {command} {json.dumps(args)} as sequence {sequence}')
        return sequence

    def open_all(self):
        """Download and open every result not yet opened; each must be for this
        chain and host, at the sequence its name gives, under its name's digest."""
        require(self.paper and self.signer, 'opening results needs the history code paper and dytallix-root-sign')
        for key in self.store.list(self.results):
            found = RESULT_NAME.match(key[len(self.results):])
            if not found or int(found.group(1)) in self.opened:
                continue
            sequence = int(found.group(1))
            status, raw = self.store.get(key)
            require(status == 200 and raw is not None, f'GET {key} answered HTTP {status}')
            require(hashlib.sha256(raw).hexdigest() == found.group(2), f'{key}: the copy does not match its name')
            sealed, opened = self.scratch / f'{sequence:010}.bin', self.scratch / f'{sequence:010}.json'
            sealed.write_bytes(raw)
            opened.unlink(missing_ok=True)
            done = subprocess.run([str(self.signer), 'history-open', '-paper', str(self.paper), '-in', str(sealed),
                                   '-out', str(opened)], capture_output=True, text=True, timeout=120)
            require(done.returncode == 0, f'history-open {key}: {done.stderr.strip()}')
            result = json.loads(opened.read_bytes())
            require(result.get('chain_id') == self.chain and result.get('host') == self.host
                    and result.get('sequence') == sequence, f'{key} is a result for {result.get("host")} '
                    f'on {result.get("chain_id")} at {result.get("sequence")}')
            self.opened[sequence] = result
            self.names[sequence] = {'object': key, 'sha256': found.group(2)}
        return self.opened

    def wait(self, sequences, timeout=600, period=10):
        """The opened results of the given sequences. A result for a command
        this channel sent must carry that command file's digest."""
        deadline = self.clock() + timeout
        while True:
            self.open_all()
            if all(s in self.opened for s in sequences):
                break
            require(self.clock() < deadline, f'{self.host}: no result for sequence(s) '
                    f'{[s for s in sequences if s not in self.opened]} within {timeout} s')
            self.sleep(period)
        for sequence in sequences:
            if sequence in self.sent:
                require(self.opened[sequence].get('command_sha256') == self.sent[sequence],
                        f'{self.host}: result {sequence} is for another command file')
        return [self.opened[s] for s in sequences]

    def run(self, command, args=None, timeout=600):
        return self.wait([self.send(command, args)], timeout)[0]


# The status page

def status_page(url, timeout=5, clock=time.time):
    """One reading of the public status page: the HTTP status, its fields and
    the reader's clock. 503 still carries the fields; no answer is None."""
    read_at = clock()
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:
            code, raw = response.status, response.read(4096)
    except urllib.error.HTTPError as error:
        code, raw = error.code, error.read(4096)
    except (urllib.error.URLError, OSError):
        return {'http': None, 'read_at': iso(read_at)}
    try:
        page = json.loads(raw)
        fields = {k: page[k] for k in STATUS_FIELDS}
        fields['height'] = int(fields['height'])
    except (ValueError, KeyError, TypeError):
        fields = {}
    return dict(fields, http=code, read_at=iso(read_at))


def block_instant(page):
    """The status page's block time in seconds since the epoch, or None."""
    text = page.get('time')
    if not isinstance(text, str):
        return None
    # The engine prints RFC 3339 with up to nanoseconds; keep microseconds.
    found = re.match(r'^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d)(\.\d+)?(Z|[+-]\d\d:\d\d)$', text)
    if not found:
        return None
    fraction = (found.group(2) or '.0')[:7]
    zone = '+00:00' if found.group(3) == 'Z' else found.group(3)
    return datetime.fromisoformat(f'{found.group(1)}{fraction}{zone}').timestamp()


def wait_caught_up(url, cleared_at, above, timeout, clock=time.time, sleep=time.sleep, period=1):
    """Poll the status page until it serves a block made at or after the
    fault cleared (host clock) and above the height read at the clear. Returns
    (the reader's instant, the height) or (None, None) after the timeout."""
    deadline = clock() + timeout
    while clock() < deadline:
        page = status_page(url, clock=clock)
        made = block_instant(page)
        if page.get('http') == 200 and made is not None and made >= cleared_at and page['height'] > above:
            return page['read_at'], page['height']
        sleep(period)
    return None, None


def preflight(store, chain, host, status_url=None, say=print):
    """Read-only checks from the founder's machine, then the checks only the
    founder can make."""
    commands = f'{store.store["commands_prefix"]}{chain}/{host}/'
    results = f'{store.store["results_prefix"]}{chain}/agent/{host}/'
    say(f'== the founder key lists {commands}: {len(store.list(commands))} command(s)')
    say(f'== the founder key lists {results}: {len(store.list(results))} result(s)')
    if status_url:
        say(f'== status page: {json.dumps(status_page(status_url))}')
    say('\n'.join([
        'Founder checks, before the first command (record each):',
        '  1. B2 accepts curl\'s signing: a PUT and a GET with the founder key worked above or by hand.',
        '  2. Each host\'s read key can GET only its own commands/CHAIN/HOST/ prefix: a GET of the other',
        '     host\'s prefix and any PUT, LIST or DELETE are refused.',
        '  3. The history upload key is write-only: LIST, GET and DELETE are refused.',
        '  4. Each host\'s read key is remade for each run\'s new chain ID (the prefix names the chain).',
        '  5. The first command on each host is fault-clear, so a wrong interface name (the fault',
        '     commands use eth0) fails in seconds; its result must be DONE.',
        '  6. Every single object stays under B2\'s 5 GB single-upload limit.',
    ]))


def reconcile_hosts(store, args):
    channels = {label: Channel(store, args.chain, label, args.work / 'results' / label, args.cli, args.home,
                               args.key, args.paper, args.signer) for label in args.host}
    client = None
    if args.client_home:
        client = reconcile.Client(args.cli, dict(os.environ, HOME=str(args.client_home)), args.work)
    load = lambda path: json.loads(path.read_bytes()) if path else None  # noqa: E731
    record = reconcile.collect_through_agent(
        args.chain, channels, args.genesis.read_bytes(), client=client, addresses=load(args.addresses) or (),
        since=load(args.since), ledger=load(args.ledger))
    args.out.write_text(json.dumps(record, indent=2) + '\n')
    for row in record['checks']:
        print(f'{row["status"]:<12} {row["check"]}: {row["detail"]}')
    print(f'== reconciliation at height {record["height"]}: {record["status"]} (evidence sha256 {record["sha256"]})')
    return 0 if record['status'] == reconcile.PASS else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('send', 'open', 'preflight'):
        p = commands.add_parser(name)
        p.add_argument('--store', type=Path, required=True)
        p.add_argument('--chain', required=True)
        p.add_argument('--host', required=True)
        if name in ('send', 'open'):
            p.add_argument('--paper', type=Path)
            p.add_argument('--signer', type=Path)
            p.add_argument('--work', type=Path, required=True)
        if name == 'send':
            p.add_argument('--cli', type=Path, required=True)
            p.add_argument('--home', type=Path, required=True)
            p.add_argument('--key', type=Path, required=True)
            p.add_argument('--command', dest='name', required=True)
            p.add_argument('--arg', action='append')
            p.add_argument('--wait', type=int, default=0)
        if name == 'preflight':
            p.add_argument('--status-url')
    r = commands.add_parser('reconcile')
    for flag in ('--store', '--cli', '--home', '--key', '--paper', '--signer', '--work', '--genesis', '--out'):
        r.add_argument(flag, type=Path, required=True)
    r.add_argument('--chain', required=True)
    r.add_argument('--host', action='append', required=True)
    r.add_argument('--client-home', type=Path)
    r.add_argument('--addresses', type=Path, help='a JSON list of the tracked addresses')
    r.add_argument('--since', type=Path)
    r.add_argument('--ledger', type=Path)
    s = commands.add_parser('status-page')
    s.add_argument('--url', required=True)
    args = parser.parse_args()
    try:
        if args.command == 'status-page':
            print(json.dumps(status_page(args.url)))
            return 0
        store = Store(load_store(args.store), Path(getattr(args, 'work', None) or '.') / 'store-scratch')
        if args.command == 'preflight':
            preflight(store, args.chain, args.host, args.status_url)
            return 0
        if args.command == 'reconcile':
            return reconcile_hosts(store, args)
        if args.command == 'send':
            channel = Channel(store, args.chain, args.host, args.work, args.cli, args.home, args.key,
                              args.paper, args.signer)
            sequence = channel.send(args.name, staging_command.parse_args(args.arg))
            if args.wait:
                print(json.dumps(channel.wait([sequence], args.wait)[0], indent=1))
            return 0
        channel = Channel(store, args.chain, args.host, args.work, paper=args.paper, signer=args.signer)
        for sequence, result in sorted(channel.open_all().items()):
            print(json.dumps({'sequence': sequence, 'command': result['command'], 'status': result['status'],
                              'reason': result['reason'], 'finished_at': result.get('finished_at')}))
    except (Failed, reconcile.Failed, staging_command.Failed, agent.Failed, OSError, KeyError, ValueError,
            subprocess.TimeoutExpired) as error:
        print(f'staging_founder: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
