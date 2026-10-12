#!/usr/bin/env python3
"""The staging agent (simulation plan, T2; node/docs/architecture/
staging-agent-v1.md). While the production hosts serve as staging, it runs
the founder's signed commands on these console-only hosts. It is never part
of a release or a host bundle, and host_install.py's wipe removes it before
the production install, whose install refuses a host that still has it.

  agent.py install --settings FILE   as root at the console, after the node's
                                     install: places the agent, its allowlist,
                                     the settings and its timer
  agent.py poll                      the dytallix-agent unit, every minute
  agent.py remove                    as root: removes the agent

Each poll fetches the next command file, by sequence number, from the
command store (an S3-compatible bucket, AWS Signature V4, a read key for one
prefix) with the host's curl, outbound only. A command runs only if it is
well formed, its ML-DSA-65 signature verifies (with the release's own
`dytallix crypto verify`) under the staging command key pinned in the
settings, it names this host, this chain and the expected sequence, it has
not expired, and its command and arguments are on the fixed allowlist
(ALLOWLIST.json). The slot is spent before anything runs, so a command runs
at most once. Every outcome, refusals included, becomes a result that is
encrypted like the metric history (`dytallix-root-sign history-seal` under
the chain's history code) and uploaded with the history upload key. The
store's keys reach curl on its standard input, never on a command line.
Standard library only; Ubuntu 24.04's python3.
"""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
from urllib.parse import urlsplit

SETTINGS_SCHEMA = 'dytallix.staging-agent-settings.v1'
ALLOWLIST_SCHEMA = 'dytallix.staging-agent-allowlist.v1'
COMMAND_SCHEMA = 'dytallix.staging-command.v1'
RESULT_SCHEMA = 'dytallix.staging-result.v1'
STATE_SCHEMA = 'dytallix.staging-agent-state.v1'
UPLOAD_SCHEMA = 'dytallix.backup-upload.v1'
# The signed message: a domain tag and the SHA-256 of the canonical body.
DOMAIN = 'dytallix-staging-command-v1:'
OPT = '/opt/dytallix-agent'
ETC = '/etc/dytallix-agent'
STATE = '/var/lib/dytallix-agent'
SETTINGS = f'{ETC}/settings.json'
UNIT = 'dytallix-agent'
UNIT_FILE = f'/etc/systemd/system/{UNIT}.service'
TIMER_FILE = f'/etc/systemd/system/{UNIT}.timer'
NODE_UNIT_FILE = '/etc/systemd/system/dytallix-node.service'
TREES = (OPT, ETC, STATE)
FILES = (UNIT_FILE, TIMER_FILE)
# Bounds.
MAX_COMMAND_BYTES = 65536
MAX_OUTPUT_BYTES = 65536
MAX_VALIDITY_SECONDS = 24 * 3600
MAX_SEQUENCE = 10**9
MAX_PER_POLL = 16
POLL_BUDGET_SECONDS = 600
PUBLIC_KEY_HEX = 2 * 1952
SIGNATURE_HEX = 2 * 3309
NAME = re.compile(r'^[A-Za-z0-9._-]{1,64}$')
COMMAND = re.compile(r'^[a-z0-9-]{1,32}$')
HEX = re.compile(r'^[0-9a-f]+$')
# Values that go into curl's configuration unquoted-safe.
SAFE = re.compile(r'^[A-Za-z0-9._~+/=-]{1,256}$')
PREFIX = re.compile(r'^([A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*/)?$')
PLACEHOLDER = re.compile(r'\{([a-z_]+)\}')
BODY_FIELDS = ('schema', 'chain_id', 'host', 'sequence', 'not_after', 'command', 'args')
STORE_FIELDS = ('endpoint', 'bucket', 'region', 'prefix', 'access_key_id', 'secret_access_key')
ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C.UTF-8'}
UNIT_TEXT = f'''[Unit]
Description=Dytallix staging agent (staging only; wiped before the production install)
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/bin/python3 -I -B {OPT}/agent.py poll
TimeoutStartSec=3600
'''
TIMER_TEXT = '''[Unit]
Description=Poll for signed staging commands every minute

[Timer]
OnBootSec=60
OnUnitInactiveSec=60
AccuracySec=5

[Install]
WantedBy=timers.target
'''


class Failed(Exception):
    pass


class Refused(Exception):
    """A command that is not run; the reason goes into its result."""


def require(ok, message, error=Failed):
    if not ok:
        raise error(message)


class Host:
    """The file system under ROOT ('/' on a host; a directory in tests)."""

    def __init__(self, root='/', run=subprocess.run):
        self.root = Path(root)
        self.run = run

    def path(self, absolute):
        return self.root / str(absolute).lstrip('/')


# Parsing

def strict_json(raw, error=Refused):
    """JSON with no duplicate keys, floats or non-finite numbers."""
    def pairs(items):
        keys = [key for key, _ in items]
        require(len(set(keys)) == len(keys), 'a key appears twice', error)
        return dict(items)

    def reject(value):
        raise error(f'{value} is not allowed')

    try:
        return json.loads(raw.decode('utf-8'), object_pairs_hook=pairs, parse_float=reject, parse_constant=reject)
    except (UnicodeDecodeError, ValueError) as failure:
        raise error(f'not JSON: {failure}') from failure


def read_json(path, limit=65536, error=Failed):
    raw = Path(path).read_bytes()
    require(len(raw) <= limit, f'{path} is too large', error)
    return strict_json(raw, error)


def is_int(value):
    return type(value) is int


def canonical(body):
    return json.dumps(body, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()


def message(body):
    """The message the staging command key signs."""
    return DOMAIN + hashlib.sha256(canonical(body)).hexdigest()


def check_store(store, what):
    require(type(store) is dict and set(store) >= set(STORE_FIELDS), f'the {what} lacks a field')
    endpoint = urlsplit(store['endpoint'])
    # TLS is only transport; plain HTTP is allowed only to a stand-in store
    # on this host.
    require(endpoint.scheme == 'https' or (endpoint.scheme == 'http' and endpoint.hostname in ('127.0.0.1', 'localhost')),
            f'the {what}\'s endpoint must use https')
    require(endpoint.path in ('', '/') and not endpoint.query and not endpoint.username,
            f'the {what}\'s endpoint is a bare origin')
    for field in ('bucket', 'region', 'access_key_id', 'secret_access_key'):
        require(type(store[field]) is str and SAFE.match(store[field]), f'the {what}\'s {field} has unexpected characters')
    require(type(store['prefix']) is str and PREFIX.match(store['prefix']), f'the {what}\'s prefix is empty or ends with /')
    return store


def check_settings(settings):
    require(type(settings) is dict and settings.get('schema') == SETTINGS_SCHEMA, f'not agent settings ({SETTINGS_SCHEMA})')
    require(set(settings) == {'schema', 'label', 'chain_id', 'command_public_key_hex', 'commands'},
            'the settings have unexpected fields')
    for field in ('label', 'chain_id'):
        require(type(settings[field]) is str and NAME.match(settings[field]), f'the settings\' {field} is not a name')
    key = settings['command_public_key_hex']
    require(type(key) is str and len(key) == PUBLIC_KEY_HEX and HEX.match(key),
            'the command key is not an ML-DSA-65 public key in lowercase hex')
    check_store(settings['commands'], 'command store key')
    return settings


def check_allowlist(allowlist):
    require(type(allowlist) is dict and allowlist.get('schema') == ALLOWLIST_SCHEMA, 'not the agent allowlist')
    commands = allowlist.get('commands')
    require(type(commands) is dict and commands, 'the allowlist names no command')
    for name, entry in commands.items():
        require(COMMAND.match(name), f'{name} is not a command name')
        require(set(entry) <= {'purpose', 'args', 'timeout_seconds', 'steps', 'stub'}, f'{name} has unexpected fields')
        require(is_int(entry.get('timeout_seconds')) and 1 <= entry['timeout_seconds'] <= 900, f'{name}: timeout')
        for arg, spec in entry['args'].items():
            require(re.fullmatch(r'[a-z_]{1,32}', arg) and spec.get('type') == 'int' and is_int(spec.get('min'))
                    and is_int(spec.get('max')) and spec['min'] <= spec['max'], f'{name}: argument {arg}')
        require(entry.get('stub') is True or entry['steps'], f'{name} has no steps')
        for step in entry['steps']:
            require(type(step) is list and step and all(type(word) is str for word in step), f'{name}: a step')
            for word in step:
                for token in PLACEHOLDER.findall(word):
                    require(token == 'bin' or token in entry['args'], f'{name}: unknown placeholder {token}')
    return allowlist


def check_command(raw, settings, allowlist, expected, now, verify):
    """The command's body and its allowlist entry, or Refused. The order:
    its shape, then its signature, then what it is bound to, then the
    allowlist."""
    require(len(raw) <= MAX_COMMAND_BYTES, f'larger than {MAX_COMMAND_BYTES} bytes', Refused)
    signed = strict_json(raw)
    require(type(signed) is dict and set(signed) == {'body', 'signature'}, 'not a signed command', Refused)
    body, signature = signed['body'], signed['signature']
    require(type(body) is dict and set(body) == set(BODY_FIELDS) and body['schema'] == COMMAND_SCHEMA,
            f'not a command ({COMMAND_SCHEMA})', Refused)
    require(all(type(body[f]) is str and NAME.match(body[f]) for f in ('chain_id', 'host')), 'malformed names', Refused)
    require(is_int(body['sequence']) and 1 <= body['sequence'] <= MAX_SEQUENCE and is_int(body['not_after']),
            'malformed sequence or expiry', Refused)
    require(type(body['command']) is str and COMMAND.match(body['command']), 'malformed command name', Refused)
    require(type(body['args']) is dict and all(re.fullmatch(r'[a-z_]{1,32}', k) and is_int(v)
                                               for k, v in body['args'].items()), 'malformed arguments', Refused)
    require(type(signature) is str and len(signature) == SIGNATURE_HEX and HEX.match(signature),
            'the signature is not an ML-DSA-65 signature in lowercase hex', Refused)
    require(verify(message(body), signature, settings['command_public_key_hex']),
            'the signature does not verify under the staging command key', Refused)
    require(body['chain_id'] == settings['chain_id'], f'signed for chain {body["chain_id"]}', Refused)
    require(body['host'] == settings['label'], f'signed for host {body["host"]}', Refused)
    require(body['sequence'] == expected, f'signed as sequence {body["sequence"]}, expected {expected}', Refused)
    require(body['not_after'] > now, 'expired', Refused)
    require(body['not_after'] - now <= MAX_VALIDITY_SECONDS, 'valid for more than 24 hours', Refused)
    entry = allowlist['commands'].get(body['command'])
    require(entry is not None, f'{body["command"]} is not on the allowlist', Refused)
    require(set(body['args']) == set(entry['args']), f'{body["command"]} takes {sorted(entry["args"])}', Refused)
    for name, value in body['args'].items():
        spec = entry['args'][name]
        require(spec['min'] <= value <= spec['max'], f'{name} is outside {spec["min"]} to {spec["max"]}', Refused)
    return body, entry


# The host

def active_release(host):
    """The release the node's unit runs, from its ExecStart."""
    found = re.search(r'^ExecStart=/opt/dytallix/([0-9a-f]{16})/bin/', host.path(NODE_UNIT_FILE).read_text(), re.M)
    require(found, 'the node\'s unit names no installed release')
    return found.group(1)


def node_config(host, settings):
    """The release's bin directory and the monitor configuration, which
    names the curl, the history code and the history upload key."""
    release = active_release(host)
    monitor = read_json(host.path(f'/etc/dytallix/{release}/monitor.json'))
    require(monitor.get('label') == settings['label'] and monitor.get('chain_id') == settings['chain_id'],
            f'the settings are for {settings["label"]} on {settings["chain_id"]}, this host is {monitor.get("label")} '
            f'on {monitor.get("chain_id")}')
    require(monitor.get('history_upload'), 'this host has no history secrets to encrypt and upload results')
    return f'/opt/dytallix/{release}/bin', monitor


def make_verify(host, bin_dir):
    """Verification with the release's own CLI; a key from the settings."""
    cli = str(host.path(f'{bin_dir}/dytallix'))
    home = host.path(f'{STATE}/cli-home')

    def verify(text, signature, public_key):
        result = host.run([cli, 'crypto', 'verify', text, signature, public_key], capture_output=True,
                          env=dict(ENV, HOME=str(home)), timeout=60)
        return result.returncode == 0
    return verify


def store_url(store, key):
    return f'{store["endpoint"].rstrip("/")}/{store["bucket"]}/{store["prefix"]}{key}'


def curl_config(store, lines):
    return '\n'.join(['silent', 'show-error', 'max-time = 60',
                      f'aws-sigv4 = "aws:amz:{store["region"]}:s3"',
                      f'user = "{store["access_key_id"]}:{store["secret_access_key"]}"', *lines, '']).encode()


def fetch(host, curl, store, key, target):
    """(HTTP status, bytes) of one object; a 404 is (404, None)."""
    target.unlink(missing_ok=True)
    result = host.run([curl, '--config', '-'], capture_output=True, input=curl_config(store, [
        f'max-filesize = {MAX_COMMAND_BYTES}', f'output = "{target}"', 'write-out = "%{http_code}"',
        f'url = "{store_url(store, key)}"']))
    if result.returncode == 63:
        return 413, None
    require(result.returncode == 0, f'the command store did not answer: '
                                    f'{(result.stderr or b"").decode(errors="replace").strip()}')
    status = int((result.stdout or b'0').decode().strip() or 0)
    raw = target.read_bytes() if status == 200 else None
    target.unlink(missing_ok=True)
    return status, raw


def truncate(raw):
    text = (raw or b'').decode(errors='replace')
    return text if len(text) <= MAX_OUTPUT_BYTES else text[:MAX_OUTPUT_BYTES] + '\n[truncated]'


def execute(host, entry, args, bin_dir):
    """Runs an allowlisted command's fixed steps, each as an argument list
    with no shell. A stub runs nothing."""
    if entry.get('stub'):
        return 'NOT_BUILT', []
    values = dict({k: str(v) for k, v in args.items()}, bin=bin_dir)
    steps = []
    for template in entry['steps']:
        argv = [PLACEHOLDER.sub(lambda m: values[m.group(1)], word) for word in template]
        try:
            result = host.run(argv, capture_output=True, env=ENV, timeout=entry['timeout_seconds'])
            steps.append({'argv': argv, 'exit': result.returncode, 'stdout': truncate(result.stdout),
                          'stderr': truncate(result.stderr)})
        except subprocess.TimeoutExpired:
            steps.append({'argv': argv, 'exit': None, 'error': f'timed out after {entry["timeout_seconds"]} s'})
        except OSError as error:
            steps.append({'argv': argv, 'exit': None, 'error': str(error)})
    return ('DONE' if all(step['exit'] == 0 for step in steps) else 'FAILED'), steps


# State and results

def load_state(host, chain):
    path = host.path(f'{STATE}/state.json')
    if path.exists():
        state = read_json(path)
        require(state.get('schema') == STATE_SCHEMA and is_int(state.get('next_sequence')), 'the agent state is damaged')
        # Never back to sequence 1: a new chain is a new install.
        require(state.get('chain_id') == chain, f'the agent state is for {state.get("chain_id")}; reinstall the agent')
        return state
    return {'schema': STATE_SCHEMA, 'chain_id': chain, 'next_sequence': 1}


def save(path, raw, mode=0o600):
    temporary = path.with_name(path.name + '.new')
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, mode)
    with os.fdopen(descriptor, 'wb') as file:
        file.write(raw)
        file.flush()
        os.fsync(file.fileno())
    os.replace(temporary, path)


def upload_results(host, monitor, now):
    """Encrypts and uploads every waiting result, oldest first; a failure
    leaves it for the next poll."""
    outbox = host.path(f'{STATE}/outbox')
    paths = monitor['history_upload']
    upload = check_store(read_json(host.path(paths['upload_file'])), 'history upload key')
    require(upload.get('schema') == UPLOAD_SCHEMA, 'not an upload key')
    uploaded = []
    day = datetime.datetime.fromtimestamp(now, datetime.timezone.utc).date().isoformat()
    for result in sorted(outbox.glob('[0-9]' * 10 + '.json')):
        copy = host.path(f'{STATE}/scratch/{result.stem}.bin')
        for leftover in (copy, copy.with_name(copy.name + '.partial')):
            leftover.unlink(missing_ok=True)
        try:
            sealed = host.run([str(host.path(paths['signer'])), 'history-seal', '-code-file',
                               str(host.path(paths['code_file'])), '-host', monitor['label'], '-day', day,
                               '-in', str(result), '-out', str(copy)], capture_output=True)
            require(sealed.returncode == 0, f'history-seal failed: {(sealed.stderr or b"").decode(errors="replace").strip()}')
            digest = hashlib.sha256(copy.read_bytes()).hexdigest()
            key = f'{monitor["chain_id"]}/agent/{monitor["label"]}/{result.stem}-{digest}.bin'
            sent = host.run([monitor['curl'], '--config', '-'], capture_output=True, input=curl_config(upload, [
                'fail', 'retry = 3', f'upload-file = "{copy}"', f'url = "{store_url(upload, key)}"']))
            require(sent.returncode == 0, f'the upload of result {result.stem} failed: '
                                          f'{(sent.stderr or b"").decode(errors="replace").strip()}')
            result.unlink()
            uploaded.append(upload['prefix'] + key)
        finally:
            copy.unlink(missing_ok=True)
    return uploaded


def poll(host, clock=time.time, out=print):
    settings = check_settings(read_json(host.path(SETTINGS)))
    allowlist = check_allowlist(read_json(host.path(f'{OPT}/ALLOWLIST.json')))
    bin_dir, monitor = node_config(host, settings)
    verify = make_verify(host, bin_dir)
    state_path = host.path(f'{STATE}/state.json')
    state = load_state(host, settings['chain_id'])
    errors, handled, uploaded = [], [], []
    # Results an earlier poll could not upload go first.
    try:
        uploaded += upload_results(host, monitor, clock())
    except (Failed, OSError, KeyError) as error:
        errors.append(str(error))
    started = clock()
    for _ in range(MAX_PER_POLL):
        if clock() - started > POLL_BUDGET_SECONDS:
            break
        sequence = state['next_sequence']
        key = f'{settings["chain_id"]}/{settings["label"]}/{sequence:010}.json'
        status, raw = fetch(host, monitor['curl'], settings['commands'], key, host.path(f'{STATE}/scratch/command'))
        if status == 404:
            break
        require(status in (200, 413), f'the command store answered {status} for {key}')
        # The slot is spent before anything runs: a command runs at most once.
        state['next_sequence'] = sequence + 1
        save(state_path, json.dumps(state).encode())
        received = clock()
        result = {'schema': RESULT_SCHEMA, 'chain_id': settings['chain_id'], 'host': settings['label'],
                  'sequence': sequence, 'object': key, 'received_at': int(received),
                  'command_sha256': hashlib.sha256(raw).hexdigest() if raw is not None else None,
                  'command': None, 'args': None, 'status': None, 'reason': None, 'steps': []}
        try:
            require(raw is not None, f'larger than {MAX_COMMAND_BYTES} bytes', Refused)
            body, entry = check_command(raw, settings, allowlist, sequence, received, verify)
            result['command'], result['args'] = body['command'], body['args']
            result['status'], result['steps'] = execute(host, entry, body['args'], bin_dir)
        except Refused as refusal:
            result['status'], result['reason'] = 'REFUSED', str(refusal)
        result['finished_at'] = int(clock())
        save(host.path(f'{STATE}/outbox/{sequence:010}.json'), json.dumps(result, indent=1).encode())
        handled.append({k: result[k] for k in ('sequence', 'command', 'status', 'reason')})
    try:
        uploaded += upload_results(host, monitor, clock())
    except (Failed, OSError, KeyError) as error:
        errors.append(str(error))
    summary = {'status': 'POLLED', 'next_sequence': state['next_sequence'], 'handled': handled,
               'uploaded': uploaded, 'errors': errors}
    out(json.dumps(summary))
    return summary


# Install and remove

def installed(host):
    return [p for p in TREES + FILES if os.path.lexists(host.path(p))]


def install(host, settings_path, source=Path(__file__).resolve().parent, out=print):
    require(os.geteuid() == 0 or str(host.root) != '/', 'run as root')
    require(host.path(NODE_UNIT_FILE).exists(), 'install the node first (install.sh); the agent uses its release')
    found = installed(host)
    require(not found, f'the agent is already installed ({", ".join(found)}); remove it first')
    raw = Path(settings_path).read_bytes()
    require(len(raw) <= 65536, 'the settings file is too large')
    settings = check_settings(strict_json(raw, Failed))
    bin_dir, monitor = node_config(host, settings)
    require(host.path(f'{bin_dir}/dytallix').is_file(), f'the release has no {bin_dir}/dytallix to verify signatures')
    agent, allowlist = (source / 'agent.py').read_bytes(), (source / 'ALLOWLIST.json').read_bytes()
    check_allowlist(strict_json(allowlist, Failed))
    os.umask(0o077)
    for tree, mode in ((OPT, 0o755), (ETC, 0o700), (STATE, 0o700), (f'{STATE}/outbox', 0o700),
                       (f'{STATE}/scratch', 0o700), (f'{STATE}/cli-home', 0o700)):
        host.path(tree).mkdir(mode=mode)
        os.chmod(host.path(tree), mode)
    for path, data, mode in ((f'{OPT}/agent.py', agent, 0o555), (f'{OPT}/ALLOWLIST.json', allowlist, 0o444),
                             (SETTINGS, raw, 0o400), (UNIT_FILE, UNIT_TEXT.encode(), 0o444),
                             (TIMER_FILE, TIMER_TEXT.encode(), 0o444)):
        target = host.path(path)
        target.write_bytes(data)
        os.chmod(target, mode)
    host.run(['systemctl', 'daemon-reload'], check=True)
    host.run(['systemctl', 'enable', '--now', f'{UNIT}.timer'], check=True)
    key_id = hashlib.sha256(bytes.fromhex(settings['command_public_key_hex'])).hexdigest()
    out(f'Installed the staging agent for {settings["label"]} on {settings["chain_id"]}: agent.py sha256 '
        f'{hashlib.sha256(agent).hexdigest()}, allowlist sha256 {hashlib.sha256(allowlist).hexdigest()}, '
        f'command key {key_id[:16]}. It polls every minute; wipe.sh removes it.')


def remove(host, out=print):
    require(os.geteuid() == 0 or str(host.root) != '/', 'run as root')
    for unit in (f'{UNIT}.timer', f'{UNIT}.service'):
        if os.path.lexists(host.path(f'/etc/systemd/system/{unit}')):
            host.run(['systemctl', 'disable', '--now', unit], check=False)
    for path in FILES:
        if os.path.lexists(host.path(path)):
            os.unlink(host.path(path))
    host.run(['systemctl', 'daemon-reload'], check=False)
    for tree in TREES:
        if os.path.lexists(host.path(tree)):
            shutil.rmtree(host.path(tree))
    remaining = installed(host)
    require(not remaining, f'still present: {", ".join(remaining)}')
    out('Removed the staging agent.')


def main(argv):
    host = Host()
    try:
        if len(argv) == 4 and argv[1] == 'install' and argv[2] == '--settings':
            install(host, argv[3])
        elif argv[1:] == ['poll']:
            poll(host)
        elif argv[1:] == ['remove']:
            remove(host)
        else:
            print('usage: agent.py install --settings FILE | poll | remove', file=sys.stderr)
            return 2
    except (Failed, OSError, KeyError, ValueError, subprocess.CalledProcessError) as error:
        print(f'agent: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv))
