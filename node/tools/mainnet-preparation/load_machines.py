#!/usr/bin/env python3
"""The load machines (simulation plan; P01, 3 November 2026,
launch/approvals/P01_T06_LOAD_MACHINES_HETZNER_2026-11-03.json): four to
eight of Hetzner Cloud's smallest servers, each with its own address, in the
hosts' Hetzner project but never in the sentry's location, rented by the hour
only for R03, R06 and the campaign's flood runs, holding only rehearsal keys,
and destroyed after each use.

  load_machines.py check
  load_machines.py up --count N --hours H --out DIR [--type NAME] [--locations L,L,...]
  load_machines.py down --out DIR
  load_machines.py reap
  load_machines.py watchdog --out DIR

Each takes --context NAME (default dytallix) and --hcloud PATH.

check lists any machine left running and any other server in the project
without delete protection; it creates nothing. up makes a throwaway SSH key
for the run, registers it, creates the machines (each labelled
`dytallix-load=RUN`) across the locations in turn, waits until each is running
with a public IPv4 address, and writes DIR/machines.json. It starts a
detached watchdog that destroys the run's machines and key at the run's
deadline (--hours), whatever else happens; down destroys them at once (the
watchdog then stops), and reap destroys every server and SSH key labelled
`dytallix-load`. A machine is always deleted, never only powered off
(Hetzner bills a stopped server).

The hosts share the project, so deleting is guarded. Hetzner deletes by ID:
the tool lists by a label selector that may never be empty, and deletes a
server only if it carries the load label (the run's value, for a run), its
name starts with `dytallix-load-` and it has no delete protection; down
deletes only the IDs machines.json recorded. A key is deleted only by its
load label, so the account's own keys stay. up refuses to start while any
server without the load label lacks delete protection, so a mistake here
cannot reach a host.

The limits are in code: at most 8 machines, only the smallest x86 types
(cpx02, cx23, cpx12), at most 12 hours, only in nbg1, fsn1 and sin (never
hel1, the sentry's), and up refuses to start while any machine labelled
`dytallix-load` exists, so two runs never overlap and nothing is left behind
unseen. If any step of up fails, everything it made is destroyed before it
exits. The machines get no token, no funder keystore and no operator key:
only what the load workers send them later.

No token passes through this tool. It runs the hcloud CLI with the context
named on every call, so the project's token stays in hcloud's own
configuration; HCLOUD_* variables are removed from its environment, so an
environment token can never pick another project. Standard library and
hcloud only.
"""
import argparse
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

CONTEXT = 'dytallix'
TAG = 'dytallix-load'
MAX_MACHINES = 8
MAX_HOURS = 12
TYPES = ('cpx02', 'cx23', 'cpx12')
IMAGE = 'ubuntu-24.04'
# Away from the sentry (hel1); each run takes them in turn, one machine each.
LOCATIONS = ('nbg1', 'fsn1', 'sin')
CONTEXT_NAME = re.compile(r'^[A-Za-z0-9_.-]{1,64}$')
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


class Hcloud:
    """The hcloud CLI, with the context named on every call."""

    def __init__(self, context=CONTEXT, binary='hcloud', run=subprocess.run):
        require(CONTEXT_NAME.fullmatch(context or ''), f'{context!r} is not an hcloud context name')
        self.context, self.binary, self.run = context, str(binary), run
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('HCLOUD_')}

    def call(self, *args, output=True):
        command = [self.binary, '--context', self.context, *args] + (['-o', 'json'] if output else [])
        done = self.run(command, capture_output=True, text=True, env=self.env, timeout=900)
        require(done.returncode == 0, f'hcloud {" ".join(args[:2])} failed: '
                                      f'{(done.stderr or done.stdout or "").strip()[:300]}')
        return json.loads(done.stdout) if output and (done.stdout or '').strip() else None

    def servers(self, selector):
        require(selector, 'a server listing needs a label selector')
        return self.call('server', 'list', '-l', selector) or []

    def every_server(self):
        return self.call('server', 'list') or []

    def keys(self, selector):
        require(selector, 'a key listing needs a label selector')
        return self.call('ssh-key', 'list', '-l', selector) or []


def labels(item):
    return item.get('labels') or {}


def protected(server):
    return bool((server.get('protection') or {}).get('delete'))


def deletable(server, run=None):
    return (TAG in labels(server) and (run is None or labels(server)[TAG] == run)
            and server.get('name', '').startswith(f'{TAG}-') and not protected(server))


def public_ipv4(server):
    return ((server.get('public_net') or {}).get('ipv4') or {}).get('ip')


def unprotected_hosts(cloud):
    return [s['name'] for s in cloud.every_server() if TAG not in labels(s) and not protected(s)]


def check(cloud, say=print):
    left = cloud.servers(TAG)
    say(json.dumps({'context': cloud.context, 'machines_running': [m['name'] for m in left],
                    'keys_left': [k['name'] for k in cloud.keys(TAG)],
                    'servers_without_delete_protection': unprotected_hosts(cloud)}))
    return left


def plan(count, hours, server_type, locations):
    require(1 <= count <= MAX_MACHINES, f'1 to {MAX_MACHINES} machines (the approval allows 4 to 8 for a run)')
    require(0 < hours <= MAX_HOURS, f'a run holds its machines for at most {MAX_HOURS} hours')
    require(server_type in TYPES, f'{server_type} is not one of the approved smallest types {TYPES}')
    require(locations and len(set(locations)) == len(locations), 'the locations must be distinct')
    outside = sorted(set(locations) - set(LOCATIONS))
    require(not outside, f'{", ".join(outside)} is not an approved location {LOCATIONS}')
    return [(i + 1, locations[i % len(locations)]) for i in range(count)]


def destroy(cloud, run, key_id, say=print, ids=None):
    """The run's machines and its key; safe to call more than once. With ids,
    only the servers among them."""
    servers = [s for s in cloud.servers(f'{TAG}={run}') if deletable(s, run) and (ids is None or s['id'] in ids)]
    if servers:
        cloud.call('server', 'delete', *[str(s['id']) for s in servers], output=False)
    keys = [k for k in cloud.keys(f'{TAG}={run}') if k['id'] == key_id and labels(k).get(TAG) == run]
    for key in keys:
        cloud.call('ssh-key', 'delete', str(key['id']), output=False)
    say(f'== destroyed {len(servers)} machine(s) labelled {TAG}={run}' + (f' and key {key_id}' if keys else ''))


def up(cloud, out, count, hours, server_type=TYPES[0], locations=LOCATIONS, timeout=600, keygen='ssh-keygen',
       clock=time.monotonic, sleep=time.sleep, say=print, watchdog=None):
    out = Path(out)
    names = plan(count, hours, server_type, list(locations))
    unprotected = unprotected_hosts(cloud)
    require(not unprotected, f'servers without delete protection share the project ({", ".join(unprotected)}); '
                             f'protect them first (hcloud server enable-protection ID delete rebuild)')
    left = cloud.servers(TAG)
    require(not left, f'machines labelled {TAG} still exist ({", ".join(m["name"] for m in left)}); '
                      f'run reap or down first')
    offered = {entry['name'] for entry in cloud.call('server-type', 'describe', server_type).get('locations') or []
               if entry.get('available') and not entry.get('deprecation')}
    missing = sorted({location for _, location in names} - offered)
    require(not missing, f'{server_type} cannot be ordered in {", ".join(missing)} now')
    started = now()
    run = started.strftime('%Y%m%d%H%M%S')
    label = f'{TAG}={run}'
    out.mkdir(parents=True, exist_ok=True, mode=0o700)
    key_file = out / 'id_ed25519'
    subprocess.run([keygen, '-q', '-t', 'ed25519', '-N', '', '-C', f'{TAG}-{run}', '-f', str(key_file)], check=True)
    state = {'schema': 'dytallix.load-machines.v2', 'provider': 'hetzner', 'context': cloud.context, 'run': run,
             'label': label, 'type': server_type, 'started': iso(started),
             'deadline': iso(started + timedelta(hours=hours)), 'key_id': None, 'created': [], 'machines': []}
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
        key = cloud.call('ssh-key', 'create', '--name', f'{TAG}-{run}', '--public-key-from-file',
                         str(out / 'id_ed25519.pub'), '--label', label)['ssh_key']
        state['key_id'] = key['id']
        save()
        if watchdog:
            watchdog(out)
        for number, location in names:
            made = cloud.call('server', 'create', '--name', f'{TAG}-{run}-{number}', '--type', server_type,
                              '--image', IMAGE, '--location', location, '--ssh-key', str(key['id']),
                              '--label', label, '--without-ipv6')['server']
            state['created'].append(made['id'])
            save()
        deadline = clock() + timeout
        while True:
            servers = [s for s in cloud.servers(label) if s['id'] in state['created']]
            ready = [s for s in servers if s.get('status') == 'running' and public_ipv4(s)]
            if len(ready) == count:
                break
            require(clock() < deadline, f'only {len(ready)} of {count} machines running after {timeout} s')
            sleep(5)
        state['machines'] = sorted(({'name': s['name'], 'id': s['id'],
                                     'location': (s.get('location') or {}).get('name'), 'ipv4': public_ipv4(s)}
                                    for s in ready), key=lambda m: m['name'])
        require(len({m['ipv4'] for m in state['machines']}) == count, 'two machines share an address')
        save()
        say(f'== {count} machines up until {state["deadline"]}: '
            + ', '.join(f'{m["name"]} {m["location"]}' for m in state['machines']))
        return state
    except BaseException:
        destroy(cloud, run, state['key_id'], say)
        raise
    finally:
        for number, handler in previous.items():
            signal.signal(number, handler)


def down(cloud, out, say=print):
    path = Path(out) / STATE
    state = json.loads(path.read_bytes())
    destroy(cloud, state['run'], state['key_id'], say, ids=set(state['created']))
    # The watchdog reads this and stops.
    state['destroyed'] = iso(now())
    path.write_text(json.dumps(state, indent=2) + '\n')


def reap(cloud, say=print):
    """Every labelled machine and every key this tool registered."""
    servers = [s for s in cloud.servers(TAG) if deletable(s)]
    if servers:
        cloud.call('server', 'delete', *[str(s['id']) for s in servers], output=False)
    keys = [k for k in cloud.keys(TAG) if TAG in labels(k) and k.get('name', '').startswith(f'{TAG}-')]
    for key in keys:
        cloud.call('ssh-key', 'delete', str(key['id']), output=False)
    say(f'== reaped {len(servers)} machine(s) and {len(keys)} key(s)')
    return len(servers), len(keys)


def watch(cloud, out, clock=now, sleep=time.sleep, say=print):
    """Destroys the run's machines and key at its deadline, or stops once
    down has destroyed them."""
    path = Path(out) / STATE
    state = json.loads(path.read_bytes())
    deadline = datetime.strptime(state['deadline'], '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    while True:
        if 'destroyed' in json.loads(path.read_bytes()):
            say('== the run was destroyed by down; the watchdog stops')
            return
        remaining = (deadline - clock()).total_seconds()
        if remaining <= 0:
            break
        sleep(min(60, max(1, remaining)))
    destroy(cloud, state['run'], state['key_id'], say)


def start_watchdog(cloud, out):
    """A detached process that outlives this one."""
    log = open(Path(out) / 'watchdog.log', 'ab')
    subprocess.Popen([sys.executable, '-I', '-B', str(Path(__file__).resolve()), 'watchdog', '--context', cloud.context,
                      '--hcloud', cloud.binary, '--out', str(out)], stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                     start_new_session=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('check', 'up', 'down', 'reap', 'watchdog'):
        p = commands.add_parser(name)
        p.add_argument('--context', default=CONTEXT)
        p.add_argument('--hcloud', default='hcloud')
        if name in ('up', 'down', 'watchdog'):
            p.add_argument('--out', type=Path, required=True)
        if name == 'up':
            p.add_argument('--count', type=int, required=True)
            p.add_argument('--hours', type=float, required=True)
            p.add_argument('--type', default=TYPES[0])
            p.add_argument('--locations', default=','.join(LOCATIONS))
    args = parser.parse_args()
    try:
        cloud = Hcloud(args.context, args.hcloud)
        if args.command == 'check':
            check(cloud)
        elif args.command == 'up':
            up(cloud, args.out, args.count, args.hours, args.type, args.locations.split(','),
               watchdog=lambda out: start_watchdog(cloud, out))
        elif args.command == 'down':
            down(cloud, args.out)
        elif args.command == 'reap':
            reap(cloud)
        else:
            watch(cloud, args.out)
    except (Failed, OSError, KeyError, ValueError, subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
        print(f'load_machines: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
