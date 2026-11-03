#!/usr/bin/env python3
"""The load machines (simulation plan; P01, 20 October 2026,
launch/approvals/P01_T06_AGENT_RECONCILE_2026-10-20.json): four to eight of
DigitalOcean's smallest droplets, spread across regions so each has its own
address, rented by the hour only for R03, R06 and the campaign's flood runs,
holding only rehearsal keys, and destroyed after each use.

  load_machines.py check --token FILE
  load_machines.py up --token FILE --count N --hours H --out DIR [--size SLUG] [--regions R,R,...]
  load_machines.py down --token FILE --out DIR
  load_machines.py reap --token FILE
  load_machines.py watchdog --token FILE --out DIR

check reads the account and lists any machine left running; it creates
nothing. up makes a throwaway SSH key for the run, registers it, creates the
machines (each tagged `dytallix-load` and `dytallix-load-RUN`) across the
regions in turn, waits until each is active with a public IPv4 address, and
writes DIR/machines.json. It starts a detached watchdog that destroys the
run's machines and key at the run's deadline (--hours), whatever else
happens; down destroys them at once, and reap destroys every machine tagged
`dytallix-load` and every key this tool registered. A machine is always
destroyed, never only powered off.

The limits are in code: at most 8 machines, only the two smallest sizes
(s-1vcpu-512mb-10gb, s-1vcpu-1gb), at most 12 hours, and up refuses to start
while any machine tagged `dytallix-load` exists, so two runs never overlap and
nothing is left behind unseen. If any step of up fails, everything it made is
destroyed before it exits. The machines get no token, no funder keystore and
no operator key: only what the load workers send them later.

The token is a DigitalOcean personal access token with custom scopes (the
launch purchase packet lists them), in an owner-only file. It reaches curl
on its standard input, never on a command line. Standard library and curl
only.
"""
import argparse
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit

API = 'https://api.digitalocean.com'
TAG = 'dytallix-load'
KEY_PREFIX = 'dytallix-load-'
MAX_MACHINES = 8
MAX_HOURS = 12
SIZES = ('s-1vcpu-512mb-10gb', 's-1vcpu-1gb')
IMAGE = 'ubuntu-24-04-x64'
# Spread across continents; each run takes them in turn, one machine each.
REGIONS = ('nyc3', 'fra1', 'sfo3', 'sgp1', 'lon1', 'tor1', 'syd1', 'blr1')
PER_REQUEST = 10
TOKEN = re.compile(r'^dop_v1_[0-9A-Za-z]{20,256}$')
STATE = 'machines.json'


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def now():
    return datetime.now(timezone.utc)


def iso(moment):
    return moment.strftime('%Y-%m-%dT%H:%M:%SZ')


def load_token(path, standin=False):
    token = Path(path).read_text().strip()
    require(standin or TOKEN.fullmatch(token), f'{path} holds no DigitalOcean personal access token')
    require(Path(path).stat().st_mode & 0o077 == 0, f'{path} must be readable by its owner only')
    return token


class Api:
    """DigitalOcean API v2 through curl; the token goes on curl's stdin."""

    def __init__(self, token, base=API, run=subprocess.run, standin=False):
        split = urlsplit(base)
        require(split.scheme == 'https' or (standin and split.hostname in ('127.0.0.1', 'localhost')),
                'the API must be reached over https (plain http only to a local stand-in)')
        self.token, self.base, self.run = token, base.rstrip('/'), run

    def call(self, method, path, body=None, expect=(200,)):
        with tempfile.TemporaryDirectory() as scratch:
            out = Path(scratch) / 'out.json'
            lines = ['silent', 'show-error', 'max-time = 60', f'request = "{method}"',
                     f'header = "Authorization: Bearer {self.token}"', f'output = "{out}"',
                     'write-out = "%{http_code}"', f'url = "{self.base}{path}"']
            if body is not None:
                data = Path(scratch) / 'body.json'
                data.write_text(json.dumps(body))
                lines += ['header = "Content-Type: application/json"', f'data-binary = "@{data}"']
            done = self.run(['curl', '--config', '-'], input=('\n'.join(lines) + '\n').encode(), capture_output=True)
            require(done.returncode == 0, f'{method} {path}: the API did not answer: '
                                          f'{(done.stderr or b"").decode(errors="replace").strip()}')
            status = int((done.stdout or b'0').decode().strip() or 0)
            raw = out.read_bytes() if out.exists() else b''
        require(status in expect, f'{method} {path} answered HTTP {status}: {raw[:300]!r}')
        return json.loads(raw) if raw.strip() else {}

    def tagged(self, tag=TAG):
        """Every machine with the tag, following pages."""
        machines, page = [], 1
        while True:
            found = self.call('GET', f'/v2/droplets?tag_name={tag}&per_page=200&page={page}')
            machines += found.get('droplets', [])
            if not (found.get('links') or {}).get('pages', {}).get('next'):
                return machines
            page += 1

    def keys(self):
        found = self.call('GET', '/v2/account/keys?per_page=200')
        return [k for k in found.get('ssh_keys', []) if k.get('name', '').startswith(KEY_PREFIX)]


def public_ipv4(machine):
    for net in (machine.get('networks') or {}).get('v4', []):
        if net.get('type') == 'public':
            return net.get('ip_address')
    return None


def check(api, say=print):
    account = api.call('GET', '/v2/account')['account']
    left = api.tagged()
    say(json.dumps({'account_status': account.get('status'), 'droplet_limit': account.get('droplet_limit'),
                    'machines_running': [m['name'] for m in left], 'keys_left': [k['name'] for k in api.keys()]}))
    return left


def plan(count, hours, size, regions):
    require(1 <= count <= MAX_MACHINES, f'1 to {MAX_MACHINES} machines (the approval allows 4 to 8 for a run)')
    require(0 < hours <= MAX_HOURS, f'a run holds its machines for at most {MAX_HOURS} hours')
    require(size in SIZES, f'{size} is not one of the approved smallest sizes {SIZES}')
    require(len(set(regions)) == len(regions) and regions, 'the regions must be distinct')
    return [(f'{TAG}-{i + 1}', regions[i % len(regions)]) for i in range(count)]


def destroy(api, run_tag, key_id, say=print):
    """The run's machines and its key; safe to call more than once."""
    api.call('DELETE', f'/v2/droplets?tag_name={run_tag}', expect=(204, 404))
    if key_id:
        api.call('DELETE', f'/v2/account/keys/{key_id}', expect=(204, 404))
    say(f'== destroyed the machines tagged {run_tag}' + (f' and key {key_id}' if key_id else ''))


def up(api, out, count, hours, size=SIZES[0], regions=REGIONS, timeout=600, keygen='ssh-keygen',
       clock=time.monotonic, sleep=time.sleep, say=print, watchdog=None):
    out = Path(out)
    names = plan(count, hours, size, list(regions))
    left = api.tagged()
    require(not left, f'machines tagged {TAG} still exist ({", ".join(m["name"] for m in left)}); '
                      f'run reap or down first')
    available = {r['slug'] for r in api.call('GET', '/v2/regions?per_page=200')['regions']
                 if r.get('available') and size in r.get('sizes', [])}
    missing = sorted({region for _, region in names} - available)
    require(not missing, f'{size} is not available in {", ".join(missing)}')
    started = now()
    run = started.strftime('%Y%m%d%H%M%S')
    run_tag = f'{TAG}-{run}'
    out.mkdir(parents=True, exist_ok=True, mode=0o700)
    key_file = out / 'id_ed25519'
    subprocess.run([keygen, '-q', '-t', 'ed25519', '-N', '', '-C', run_tag, '-f', str(key_file)], check=True)
    state = {'schema': 'dytallix.load-machines.v1', 'run': run, 'tag': run_tag, 'size': size,
             'started': iso(started), 'deadline': iso(started + timedelta(hours=hours)), 'key_id': None,
             'machines': []}
    path = out / STATE

    def save():
        path.write_text(json.dumps(state, indent=2) + '\n')

    previous = {}

    def on_signal(number, _frame):
        raise Failed(f'stopped by signal {number}')
    for number in (signal.SIGINT, signal.SIGTERM):
        previous[number] = signal.signal(number, on_signal)
    try:
        save()
        key = api.call('POST', '/v2/account/keys', {'name': f'{KEY_PREFIX}{run}',
                                                     'public_key': (out / 'id_ed25519.pub').read_text().strip()},
                       expect=(201,))['ssh_key']
        state['key_id'] = key['id']
        save()
        if watchdog:
            watchdog(out)
        by_region = {}
        for name, region in names:
            by_region.setdefault(region, []).append(name)
        for region, group in by_region.items():
            for i in range(0, len(group), PER_REQUEST):
                api.call('POST', '/v2/droplets', {'names': group[i:i + PER_REQUEST], 'region': region, 'size': size,
                                                  'image': IMAGE, 'ssh_keys': [key['id']], 'tags': [TAG, run_tag],
                                                  'ipv6': False, 'monitoring': False}, expect=(202,))
        deadline = clock() + timeout
        while True:
            machines = api.tagged(run_tag)
            ready = [m for m in machines if m.get('status') == 'active' and public_ipv4(m)]
            if len(ready) == count:
                break
            require(clock() < deadline, f'only {len(ready)} of {count} machines active after {timeout} s')
            sleep(5)
        state['machines'] = sorted(({'name': m['name'], 'id': m['id'], 'region': (m.get('region') or {}).get('slug'),
                                     'ipv4': public_ipv4(m)} for m in ready), key=lambda m: m['name'])
        require(len({m['ipv4'] for m in state['machines']}) == count, 'two machines share an address')
        save()
        say(f'== {count} machines up until {state["deadline"]}: '
            + ', '.join(f'{m["name"]} {m["region"]}' for m in state['machines']))
        return state
    except BaseException:
        destroy(api, run_tag, state['key_id'], say)
        raise
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)


def down(api, out, say=print):
    state = json.loads((Path(out) / STATE).read_bytes())
    destroy(api, state['tag'], state['key_id'], say)


def reap(api, say=print):
    """Every tagged machine and every key this tool registered."""
    machines = api.tagged()
    api.call('DELETE', f'/v2/droplets?tag_name={TAG}', expect=(204, 404))
    keys = api.keys()
    for key in keys:
        api.call('DELETE', f'/v2/account/keys/{key["id"]}', expect=(204, 404))
    say(f'== reaped {len(machines)} machine(s) and {len(keys)} key(s)')
    return len(machines), len(keys)


def watch(api, out, clock=now, sleep=time.sleep, say=print):
    """Destroys the run's machines and key at its deadline."""
    state = json.loads((Path(out) / STATE).read_bytes())
    deadline = datetime.strptime(state['deadline'], '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    while True:
        remaining = (deadline - clock()).total_seconds()
        if remaining <= 0:
            break
        sleep(min(60, max(1, remaining)))
    destroy(api, state['tag'], state['key_id'], say)


def start_watchdog(token_path, out):
    """A detached process that outlives this one."""
    log = open(Path(out) / 'watchdog.log', 'ab')
    subprocess.Popen([sys.executable, '-I', '-B', str(Path(__file__).resolve()), 'watchdog', '--token', str(token_path),
                      '--out', str(out)], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('check', 'up', 'down', 'reap', 'watchdog'):
        p = commands.add_parser(name)
        p.add_argument('--token', type=Path, required=True)
        if name in ('up', 'down', 'watchdog'):
            p.add_argument('--out', type=Path, required=True)
        if name == 'up':
            p.add_argument('--count', type=int, required=True)
            p.add_argument('--hours', type=float, required=True)
            p.add_argument('--size', default=SIZES[0])
            p.add_argument('--regions', default=','.join(REGIONS))
    args = parser.parse_args()
    try:
        api = Api(load_token(args.token))
        if args.command == 'check':
            check(api)
        elif args.command == 'up':
            up(api, args.out, args.count, args.hours, args.size, args.regions.split(','),
               watchdog=lambda out: start_watchdog(args.token, out))
        elif args.command == 'down':
            down(api, args.out)
        elif args.command == 'reap':
            reap(api)
        else:
            watch(api, args.out)
    except (Failed, OSError, KeyError, ValueError, subprocess.CalledProcessError) as error:
        print(f'load_machines: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
