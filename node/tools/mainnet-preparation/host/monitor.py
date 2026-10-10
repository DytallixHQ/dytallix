#!/usr/bin/env python3
"""Watch this host and its node, and push a heartbeat and alerts outbound
(E05; monitoring v1, node/docs/architecture/monitoring-v1.md, M4). The
dytallix-monitor unit runs it every minute from its timer, as root with no
capabilities, a read-only system and its own writable state:

  monitor.py --config /etc/dytallix/RELEASE/monitor.json

Each run reads the node's two metric files, the host's CPU, memory, data
disk and network counters, the node unit's state and, on the sentry, the
backup job's last upload, and on the endpoint times one local status
request. It evaluates the approved alert rules (P01, 10 October 2026), sends
each change of an alert's state (firing or resolved) to the alert URL, a
severity 1 alert still firing after the escalation time to the escalation
URL, and a heartbeat to the heartbeat URL, all with the host's curl. A
delivery that fails is kept and retried on the next run; the heartbeat
carries the count. It appends one line to the day's history file and keeps
the approved number of days. With the history secrets (M4b) it encrypts one
finished day per run, oldest first, with the release's `dytallix-root-sign
history-seal` under the chain's history code, and uploads it with curl (AWS
Signature V4, the write-only upload key on curl's standard input). The URLs
come from the host's monitor settings
(/etc/dytallix-monitor/webhooks.json, root 0400, installed by
`host_install.py monitor-settings`); without them it still records history
and alert state, and the alerting service, receiving no heartbeat, alerts.
Standard library only.
"""
import argparse
import datetime
import gzip
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

CONFIG_SCHEMA = 'dytallix.host-monitor.v1'
SETTINGS_SCHEMA = 'dytallix.monitor-settings.v1'
STATE_SCHEMA = 'dytallix.monitor-state.v1'
ALERT_SCHEMA = 'dytallix.alert.v1'
HEARTBEAT_SCHEMA = 'dytallix.heartbeat.v1'
ROLES = ('validator', 'sentry', 'endpoint')
# A URL goes into curl's configuration between double quotes.
URL_CHARS = re.compile(r'^[A-Za-z0-9._~:/?#\[\]@!$&\'()*+,;=%-]{1,2048}$')
SAMPLE = re.compile(r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(?:\{(.*)\})?\s+(\S+)(?:\s+\S+)?$')
LABEL = re.compile(r'([a-zA-Z_][a-zA-Z0-9_]*)="((?:[^"\\]|\\.)*)"')
WRITTEN = 'dytallix_metrics_written_timestamp_seconds'
MAX_METRIC_BYTES = 1 << 20
# Samples kept for windowed rules: the longest window and one run's slack.
SAMPLE_SPAN = 3600 + 180
# Undelivered alerts kept for the next run; older ones are dropped and counted.
MAX_OUTBOX = 200
RUNBOOK = 'https://github.com/DytallixHQ/dytallix/blob/main/node/docs/operations/monitoring.md'
# The history upload key (M4b) has the backup upload key's fields.
UPLOAD_SCHEMA = 'dytallix.backup-upload.v1'
# Values that go into curl's configuration unquoted-safe.
SAFE = re.compile(r'^[A-Za-z0-9._~+/=-]{1,256}$')
FINISHED_DAY = re.compile(r'^([0-9]{4}-[0-9]{2}-[0-9]{2})\.jsonl\.gz$')

# The approved rules (P01, 10 October 2026; monitoring v1, Alerts): name,
# severity and the roles that evaluate it. Validator outage and monitoring
# down are the alerting service's: it alerts when a host's heartbeat stops.
RULES = {
    'consensus_halt': (1, ROLES),
    'missed_blocks': (2, ('validator',)),
    'disk_low': (2, ROLES),
    'disk_critical': (1, ROLES),
    'rpc_failing': (2, ('endpoint',)),
    'supply_invariant': (1, ('validator', 'sentry')),
    'unexpected_mint_burn': (1, ('validator', 'sentry')),
    'staking_change': (2, ('validator',)),
    'governance_event': (3, ('validator',)),
    'signature_failures': (2, ROLES),
    'restart_loop': (1, ROLES),
    'stale_metrics': (2, ROLES),
    'stale_backup': (2, ('sentry',)),
}
THRESHOLDS = ('halt_seconds', 'missed_blocks', 'missed_blocks_window_seconds', 'disk_free_warning_percent',
              'disk_free_critical_percent', 'rpc_failing_seconds', 'staking_change_percent', 'staking_window_seconds',
              'signature_failures', 'signature_failures_window_seconds', 'restarts', 'restarts_window_seconds',
              'metrics_max_age_seconds', 'backup_max_age_seconds', 'escalation_seconds', 'history_days')


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def read_json(path, limit=65536):
    raw = Path(path).read_bytes()
    require(len(raw) <= limit, f'{path} is too large')
    return json.loads(raw)


def check_url(url, field):
    """An https URL, or plain http only to a stand-in on this host."""
    require(isinstance(url, str) and URL_CHARS.match(url), f'the settings\' {field} has unexpected characters')
    parts = urlsplit(url)
    require(parts.scheme == 'https' or (parts.scheme == 'http' and parts.hostname in ('127.0.0.1', 'localhost')),
            f'the settings\' {field} must use https')
    require(parts.hostname and not parts.username and not parts.password, f'the settings\' {field} is not a plain URL')
    return url


def load(config_path):
    config = read_json(config_path)
    require(config.get('schema') == CONFIG_SCHEMA, 'not a host monitor configuration')
    require(config.get('role') in ROLES, 'unknown role')
    thresholds = config.get('thresholds', {})
    for name in THRESHOLDS:
        require(type(thresholds.get(name)) is int and thresholds[name] > 0, f'threshold {name} must be a positive integer')
    require(thresholds['disk_free_critical_percent'] < thresholds['disk_free_warning_percent'] <= 100,
            'the critical disk level must be below the warning level')
    require(type(config.get('emission_ceiling_udrt_per_block')) is int and config['emission_ceiling_udrt_per_block'] > 0,
            'the emission ceiling must be a positive integer')
    return config


def load_settings(config):
    """The host's webhook URLs, or None before they are installed."""
    path = Path(config['settings'])
    if not path.exists():
        return None
    settings = read_json(path)
    require(settings.get('schema') == SETTINGS_SCHEMA, 'not monitor settings')
    require(settings.get('label') == config['label'], f'the monitor settings are for {settings.get("label")}, '
                                                      f'not {config["label"]}')
    for field in ('heartbeat_url', 'alert_url'):
        check_url(settings.get(field), field)
    if settings.get('escalation_url') is not None:
        check_url(settings['escalation_url'], 'escalation_url')
    return settings


# Reading

def parse_metrics(text):
    """Prometheus text samples by (name, sorted labels); integers stay exact."""
    samples = {}
    for line in text.splitlines():
        if not line or line.startswith('#'):
            continue
        match = SAMPLE.match(line)
        if not match:
            continue
        name, labels, raw = match.groups()
        key = (name, tuple(sorted(LABEL.findall(labels or ''))))
        try:
            samples[key] = int(raw) if re.fullmatch(r'-?[0-9]+', raw) else float(raw)
        except ValueError:
            continue
    return samples


def read_metrics(path):
    try:
        with open(path, 'rb') as file:
            raw = file.read(MAX_METRIC_BYTES + 1)
    except OSError:
        return None
    if len(raw) > MAX_METRIC_BYTES:
        return None
    return parse_metrics(raw.decode('utf-8', errors='replace'))


def value(samples, name, **labels):
    if samples is None:
        return None
    return samples.get((name, tuple(sorted(labels.items()))))


def total(samples, name, **labels):
    """The sum over every label set that has the given labels."""
    if samples is None:
        return None
    found = [v for (n, ls), v in samples.items() if n == name and set(labels.items()) <= set(ls)]
    return sum(found) if found else None


def written(samples):
    if samples is None:
        return None
    found = [v for (n, _), v in samples.items() if n == WRITTEN]
    return float(found[0]) if found else None


def cpu(proc):
    fields = [int(x) for x in (Path(proc) / 'stat').read_text().splitlines()[0].split()[1:]]
    return sum(fields), fields[3] + (fields[4] if len(fields) > 4 else 0)


def memory_percent(proc):
    info = {}
    for line in (Path(proc) / 'meminfo').read_text().splitlines():
        key, _, rest = line.partition(':')
        info[key] = int(rest.split()[0])
    return round(100 * (1 - info['MemAvailable'] / info['MemTotal']), 1)


def network(proc):
    rx = tx = 0
    for line in (Path(proc) / 'net' / 'dev').read_text().splitlines()[2:]:
        name, _, rest = line.partition(':')
        if name.strip() == 'lo':
            continue
        fields = rest.split()
        rx, tx = rx + int(fields[0]), tx + int(fields[8])
    return rx, tx


def unit_state(run, systemctl, unit):
    result = run([systemctl, 'show', unit, '--property=ActiveState', '--property=InvocationID'],
                 capture_output=True, text=True)
    require(result.returncode == 0, f'systemctl show {unit} failed')
    properties = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    return properties.get('ActiveState', ''), properties.get('InvocationID', '')


def status_request(run, curl, url):
    """The local status page: (HTTP status, milliseconds), status 0 on failure."""
    started = time.monotonic()
    result = run([curl, '--config', '-'], input='\n'.join([
        'silent', 'max-time = 5', 'output = "/dev/null"', 'write-out = "%{http_code}"', f'url = "{url}"', '']),
        capture_output=True, text=True)
    millis = round((time.monotonic() - started) * 1000)
    code = result.stdout.strip()
    return (int(code) if result.returncode == 0 and code.isdigit() else 0), millis


# Rules

def window_delta(samples, field, current, now, window):
    """How much a counter rose within the window; a reset counts from zero."""
    if current is None:
        return None
    base = next((s[field] for s in samples if s['t'] >= now - window and s.get(field) is not None), None)
    if base is None:
        return 0
    return current - base if current >= base else current


def evaluate(config, state, sample, now):
    """Each rule this host evaluates: (firing, value, threshold), or None
    while it cannot be judged (its inputs missing)."""
    t = config['thresholds']
    role = config['role']
    history = state['samples']
    previous = history[-1] if history else None
    results = {}

    # Consensus halt: the application height unchanged for the halt time.
    if sample['height'] is not None:
        if sample['height'] != state.get('last_height'):
            state['last_height'], state['height_changed_at'] = sample['height'], now
        age = now - state['height_changed_at']
        results['consensus_halt'] = (age >= t['halt_seconds'], round(age), t['halt_seconds'])

    if role == 'validator':
        missed = window_delta(history, 'missed', sample['missed'], now, t['missed_blocks_window_seconds'])
        if missed is not None:
            results['missed_blocks'] = (missed >= t['missed_blocks'], missed, t['missed_blocks'])

    if sample['disk_free_percent'] is not None:
        free = sample['disk_free_percent']
        results['disk_low'] = (free < t['disk_free_warning_percent'], free, t['disk_free_warning_percent'])
        results['disk_critical'] = (free < t['disk_free_critical_percent'], free, t['disk_free_critical_percent'])

    if role == 'endpoint':
        if sample['rpc_status'] == 200:
            state['rpc_failing_since'] = None
        elif state.get('rpc_failing_since') is None:
            state['rpc_failing_since'] = now
        since = state.get('rpc_failing_since')
        failing = 0 if since is None else round(now - since)
        results['rpc_failing'] = (failing >= t['rpc_failing_seconds'], failing, t['rpc_failing_seconds'])

    if role in ('validator', 'sentry') and sample['drt_total'] is not None:
        expected = sample['drt_genesis'] + sample['emitted'] - sample['burned']
        if state.get('dgt_issued') is None:
            state['dgt_issued'] = sample['dgt_issued']
        broken = sample['drt_total'] != expected or sample['dgt_issued'] != state['dgt_issued']
        results['supply_invariant'] = (broken, sample['drt_total'] - expected, 0)
        if (previous and previous.get('emitted') is not None and previous.get('height') is not None
                and sample['height'] is not None):
            blocks = max(sample['height'] - previous['height'], 0)
            emitted = sample['emitted'] - previous['emitted']
            ceiling = config['emission_ceiling_udrt_per_block'] * blocks
            burned = sample['burned'] - previous['burned']
            # Without fees (no committed transaction) nothing burns; a node
            # restart resets the counters, so that interval is not judged.
            same_process = sample['invocation'] == previous.get('invocation')
            quiet = (same_process and sample['transactions'] is not None
                     and sample['transactions'] == previous.get('transactions'))
            unexpected = emitted < 0 or emitted > ceiling or burned < 0 or (burned > 0 and quiet)
            results['unexpected_mint_burn'] = (unexpected, {'emitted': emitted, 'burned': burned, 'blocks': blocks},
                                               ceiling)

    if role == 'validator':
        then = next((s for s in history if now - t['staking_window_seconds'] - 180 <= s['t']
                     <= now - t['staking_window_seconds'] + 180 and s.get('staked')), None)
        if then and sample['staked'] is not None:
            change = abs(sample['staked'] - then['staked']) * 100 / then['staked']
            results['staking_change'] = (change > t['staking_change_percent'], round(change, 2),
                                         t['staking_change_percent'])
        if sample['governance'] is not None and previous and previous.get('governance') is not None:
            new = sample['governance'] - previous['governance']
            new = sample['governance'] if new < 0 else new
            results['governance_event'] = (new > 0, new, 0)

    failures = window_delta(history, 'signature_invalid', sample['signature_invalid'], now,
                            t['signature_failures_window_seconds'])
    if failures is not None:
        results['signature_failures'] = (failures >= t['signature_failures'], failures, t['signature_failures'])

    starts = [s for s in state['starts'] if s >= now - t['restarts_window_seconds']]
    state['starts'] = starts
    results['restart_loop'] = (len(starts) >= t['restarts'], len(starts), t['restarts'])

    ages = [sample['app_age'], sample['engine_age']]
    stale = [a for a in ages if a is None or a > t['metrics_max_age_seconds']]
    oldest = max((a for a in ages if a is not None), default=None)
    results['stale_metrics'] = (bool(stale), None if oldest is None else round(oldest), t['metrics_max_age_seconds'])

    if role == 'sentry':
        uploaded = sample['backup_uploaded']
        age = now - (uploaded if uploaded is not None else state['first_run'])
        results['stale_backup'] = (age >= t['backup_max_age_seconds'], round(age), t['backup_max_age_seconds'])

    return {name: result for name, result in results.items() if config['role'] in RULES[name][1]}


# Sending

def curl_config(url, payload_path):
    return '\n'.join([
        'fail', 'silent', 'show-error', 'max-time = 10',
        'header = "Content-Type: application/json"',
        f'data-binary = "@{payload_path}"',
        f'url = "{url}"', '']).encode()


def send(run, curl, scratch, url, payload):
    path = Path(scratch) / 'payload.json'
    path.write_text(json.dumps(payload, separators=(',', ':')))
    try:
        result = run([curl, '--config', '-'], input=curl_config(url, path), capture_output=True)
        return result.returncode == 0
    finally:
        path.unlink(missing_ok=True)


def alert_payload(config, name, alert, now, kind):
    severity = RULES[name][0]
    return {'schema': ALERT_SCHEMA, 'kind': kind, 'status': 'firing' if alert['firing'] else 'resolved',
            'severity': severity, 'condition': name, 'host': config['label'], 'role': config['role'],
            'chain_id': config['chain_id'], 'value': alert['value'], 'threshold': alert['threshold'],
            'since': alert['since'], 'time': round(now), 'runbook': f'{RUNBOOK}#{name.replace("_", "-")}'}


# History

def history(config, state_dir, line, now):
    directory = Path(state_dir) / 'history'
    directory.mkdir(mode=0o700, exist_ok=True)
    today = datetime.datetime.fromtimestamp(now, datetime.timezone.utc).date()
    with open(directory / f'{today.isoformat()}.jsonl', 'a') as file:
        file.write(json.dumps(line, separators=(',', ':')) + '\n')
    oldest = today - datetime.timedelta(days=config['thresholds']['history_days'])
    for entry in sorted(directory.iterdir()):
        match = re.fullmatch(r'([0-9]{4}-[0-9]{2}-[0-9]{2})\.jsonl(\.gz)?', entry.name)
        if not match:
            continue
        day = datetime.date.fromisoformat(match.group(1))
        if day < oldest:
            entry.unlink()
        elif day < today and not match.group(2):
            # A finished day is compressed once.
            with open(entry, 'rb') as source, gzip.open(entry.with_name(entry.name + '.gz.partial'), 'wb') as target:
                shutil.copyfileobj(source, target)
            os.replace(entry.with_name(entry.name + '.gz.partial'), entry.with_name(entry.name + '.gz'))
            entry.unlink()


# Uploading finished days (M4b)

def load_history_upload(config):
    """The signer, code file and upload key, or None without them."""
    paths = config.get('history_upload')
    if not paths:
        return None
    upload = read_json(paths['upload_file'])
    require(upload.get('schema') == UPLOAD_SCHEMA, 'not an upload key')
    endpoint = urlsplit(upload['endpoint'])
    # TLS is only transport (the copy is encrypted and authenticated); plain
    # HTTP is allowed only to a stand-in store on this host.
    require(endpoint.scheme == 'https' or (endpoint.scheme == 'http' and endpoint.hostname in ('127.0.0.1', 'localhost')),
            'the upload endpoint must use https')
    require(endpoint.path in ('', '/') and not endpoint.query and not endpoint.username, 'the endpoint is a bare origin')
    for field in ('bucket', 'region', 'access_key_id', 'secret_access_key'):
        require(SAFE.match(upload[field] or ''), f'the upload key\'s {field} has unexpected characters')
    require(upload['prefix'] == '' or re.fullmatch(r'[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*/', upload['prefix']),
            'the prefix is empty or ends with /')
    return paths, upload


def upload_config(upload, copy, url):
    """curl's options for one upload, read from its standard input."""
    return '\n'.join([
        'fail', 'silent', 'show-error', 'max-time = 30',
        f'aws-sigv4 = "aws:amz:{upload["region"]}:s3"',
        f'user = "{upload["access_key_id"]}:{upload["secret_access_key"]}"',
        f'upload-file = "{copy}"',
        f'url = "{url}"', '']).encode()


def finished_days(state_dir, now):
    today = datetime.datetime.fromtimestamp(now, datetime.timezone.utc).date().isoformat()
    directory = Path(state_dir) / 'history'
    if not directory.exists():
        return []
    days = (FINISHED_DAY.match(entry.name) for entry in directory.iterdir())
    return sorted(match.group(1) for match in days if match and match.group(1) < today)


def upload_history(config, state, state_dir, now, run, errors):
    """Uploads the oldest finished day not yet uploaded; returns how many
    still wait, or None without the history secrets."""
    try:
        loaded = load_history_upload(config)
    except (Failed, OSError, KeyError, ValueError) as error:
        errors.append(f'history: {error}')
        return None
    if loaded is None:
        return None
    paths, upload = loaded
    pending = [day for day in finished_days(state_dir, now) if day > state.get('history_uploaded', '')]
    if not pending:
        return 0
    day = pending[0]
    scratch = Path(state_dir) / 'scratch'
    copy = scratch / f'history-{day}.bin'
    for leftover in (copy, copy.with_name(copy.name + '.partial')):
        leftover.unlink(missing_ok=True)
    try:
        result = run([paths['signer'], 'history-seal', '-code-file', paths['code_file'], '-host', config['label'],
                      '-day', day, '-in', str(Path(state_dir) / 'history' / f'{day}.jsonl.gz'), '-out', str(copy)],
                     capture_output=True, text=True)
        require(result.returncode == 0, f'history-seal failed: {(result.stderr or "").strip()}')
        digest = hashlib.sha256(copy.read_bytes()).hexdigest()
        name = f'{upload["prefix"]}{config["chain_id"]}/history/{config["label"]}/{day}-{digest}.bin'
        url = f'{upload["endpoint"].rstrip("/")}/{upload["bucket"]}/{name}'
        result = run([config['curl'], '--config', '-'], input=upload_config(upload, copy, url), capture_output=True)
        require(result.returncode == 0, f'the upload of {day} failed: '
                                        f'{(result.stderr or b"").decode(errors="replace").strip()}')
        state['history_uploaded'] = day
        return len(pending) - 1
    except (Failed, OSError) as error:
        state['history_failures'] = state.get('history_failures', 0) + 1
        errors.append(f'history: {error}')
        return len(pending)
    finally:
        copy.unlink(missing_ok=True)


# The run

def load_state(state_dir, now):
    path = Path(state_dir) / 'state.json'
    if path.exists():
        try:
            state = json.loads(path.read_bytes())
            if state.get('schema') == STATE_SCHEMA:
                return state
        except ValueError:
            pass
    return {'schema': STATE_SCHEMA, 'first_run': now, 'samples': [], 'starts': [], 'alerts': {}, 'outbox': [],
            'dropped': 0, 'delivery_failures': 0, 'last_invocation': None, 'height_changed_at': now}


def save_state(state_dir, state):
    path = Path(state_dir) / 'state.json'
    temporary = path.with_name('state.json.new')
    temporary.write_text(json.dumps(state, separators=(',', ':')))
    os.replace(temporary, path)


def collect(config, state, now, run, statvfs):
    """This run's sample; an input that cannot be read is None."""
    errors = []
    metrics = Path(config['metrics'])
    app, engine = read_metrics(metrics / 'dytallix-app.prom'), read_metrics(metrics / 'dytallix-engine.prom')
    app_written, engine_written = written(app), written(engine)
    sample = {
        't': now,
        'height': value(app, 'dytallix_app_height'),
        'app_age': None if app_written is None else now - app_written,
        'engine_age': None if engine_written is None else now - engine_written,
        'missed': total(engine, 'dytallix_engine_consensus_validator_missed_blocks'),
        'peers': value(engine, 'dytallix_engine_p2p_peers'),
        'mempool': value(engine, 'dytallix_engine_mempool_size'),
        'signature_invalid': total(app, 'dytallix_app_signature_verifications_total', result='invalid'),
        'transactions': total(app, 'dytallix_app_transactions_total'),
        'governance': total(app, 'dytallix_app_transactions_total', kind='governance'),
        'drt_total': value(app, 'dytallix_app_supply_udrt', bucket='total'),
        'drt_genesis': value(app, 'dytallix_app_supply_udrt', bucket='genesis'),
        'emitted': value(app, 'dytallix_app_supply_udrt', bucket='emitted'),
        'burned': value(app, 'dytallix_app_supply_udrt', bucket='burned'),
        'dgt_issued': value(app, 'dytallix_app_supply_udgt', bucket='issued'),
        'staked': value(app, 'dytallix_app_supply_udgt', bucket='staked'),
        'disk_free_percent': None, 'disk_used': None, 'cpu_percent': None, 'memory_percent': None,
        'rx_rate': None, 'tx_rate': None, 'node': None, 'invocation': None, 'backup_uploaded': None,
        'rpc_status': None, 'rpc_ms': None,
    }
    supply = ('drt_total', 'drt_genesis', 'emitted', 'burned', 'dgt_issued')
    if any(sample[f] is None for f in supply):
        for field in supply:
            sample[field] = None
    try:
        disk = statvfs(config['data_disk'])
        sample['disk_free_percent'] = round(100 * disk.f_bavail / disk.f_blocks, 2)
        sample['disk_used'] = (disk.f_blocks - disk.f_bfree) * disk.f_frsize
    except (OSError, ZeroDivisionError) as error:
        errors.append(f'disk: {error}')
    proc = config.get('proc', '/proc')
    try:
        busy_total, idle = cpu(proc)
        previous = state.get('cpu')
        if previous and busy_total > previous[0]:
            sample['cpu_percent'] = round(100 * (1 - (idle - previous[1]) / (busy_total - previous[0])), 1)
        state['cpu'] = [busy_total, idle]
        sample['memory_percent'] = memory_percent(proc)
        rx, tx = network(proc)
        previous = state.get('net')
        if previous and now > previous[0] and rx >= previous[1] and tx >= previous[2]:
            sample['rx_rate'] = round((rx - previous[1]) / (now - previous[0]))
            sample['tx_rate'] = round((tx - previous[2]) / (now - previous[0]))
        state['net'] = [now, rx, tx]
    except (OSError, ValueError, IndexError, KeyError, ZeroDivisionError) as error:
        errors.append(f'proc: {error}')
    try:
        sample['node'], sample['invocation'] = unit_state(run, config['systemctl'], config['node_unit'])
        if sample['invocation'] and sample['invocation'] != state.get('last_invocation'):
            # A new start of the node; the first run only learns the current one.
            if state.get('last_invocation') is not None or state['samples']:
                state['starts'].append(now)
            state['last_invocation'] = sample['invocation']
    except (Failed, OSError, ValueError) as error:
        errors.append(f'unit: {error}')
    if config.get('backup_state'):
        try:
            sample['backup_uploaded'] = os.stat(config['backup_state']).st_mtime
        except FileNotFoundError:
            pass
        except OSError as error:
            errors.append(f'backup: {error}')
    if config.get('status_url'):
        try:
            sample['rpc_status'], sample['rpc_ms'] = status_request(run, config['curl'], config['status_url'])
        except OSError as error:
            sample['rpc_status'] = 0
            errors.append(f'status: {error}')
    return sample, errors


def run_once(config_path, run=subprocess.run, clock=time.time, statvfs=os.statvfs, out=print):
    config = load(config_path)
    state_dir = Path(config['state'])
    now = clock()
    state = load_state(state_dir, now)
    sample, errors = collect(config, state, now, run, statvfs)
    try:
        settings = load_settings(config)
    except (Failed, OSError, ValueError) as error:
        settings = None
        errors.append(f'settings: {error}')
    results = evaluate(config, state, sample, now)

    # Changes of state go to the alert URL; a severity 1 alert firing past
    # the escalation time also goes to the escalation URL, once.
    alerts = state['alerts']
    for name, (firing, observed, threshold) in sorted(results.items()):
        current = alerts.get(name)
        if current is None or current['firing'] != firing:
            if current is None and not firing:
                alerts[name] = {'firing': False, 'since': round(now), 'value': observed, 'threshold': threshold,
                                'escalated': False}
                continue
            alerts[name] = {'firing': firing, 'since': round(now), 'value': observed, 'threshold': threshold,
                            'escalated': False}
            state['outbox'].append({'to': 'alert', 'payload': alert_payload(config, name, alerts[name], now, 'change')})
        else:
            current['value'], current['threshold'] = observed, threshold
        alert = alerts[name]
        if (alert['firing'] and not alert['escalated'] and RULES[name][0] == 1
                and now - alert['since'] >= config['thresholds']['escalation_seconds']):
            alert['escalated'] = True
            state['outbox'].append({'to': 'escalation',
                                    'payload': alert_payload(config, name, alert, now, 'escalation')})
    if len(state['outbox']) > MAX_OUTBOX:
        state['dropped'] += len(state['outbox']) - MAX_OUTBOX
        state['outbox'] = state['outbox'][-MAX_OUTBOX:]

    scratch = state_dir / 'scratch'
    scratch.mkdir(mode=0o700, exist_ok=True)
    delivered = 0
    if settings is not None:
        remaining = []
        for index, entry in enumerate(state['outbox']):
            url = settings['alert_url'] if entry['to'] == 'alert' else settings.get('escalation_url')
            if url is None:
                continue  # the alerting service escalates by itself
            if send(run, config['curl'], scratch, url, entry['payload']):
                delivered += 1
            else:
                # Keep order: this one and every later one wait for the next run.
                state['delivery_failures'] += 1
                remaining = state['outbox'][index:]
                break
        state['outbox'] = remaining
    firing = sorted(name for name, alert in alerts.items() if alert['firing'])
    heartbeat = {'schema': HEARTBEAT_SCHEMA, 'host': config['label'], 'role': config['role'],
                 'chain_id': config['chain_id'], 'time': round(now), 'height': sample['height'], 'firing': firing,
                 'undelivered': len(state['outbox']), 'dropped': state['dropped'],
                 'delivery_failures': state['delivery_failures'], 'history_pending': state.get('history_pending'),
                 'history_failures': state.get('history_failures', 0), 'errors': errors}
    beat = settings is not None and send(run, config['curl'], scratch, settings['heartbeat_url'], heartbeat)
    if beat and not state['outbox']:
        state['delivery_failures'] = 0

    state['samples'] = [s for s in state['samples'] if s['t'] >= now - SAMPLE_SPAN] + [
        {k: sample[k] for k in ('t', 'height', 'missed', 'signature_invalid', 'transactions', 'governance', 'staked',
                                'emitted', 'burned', 'invocation')}]
    save_state(state_dir, state)
    history(config, state_dir, {
        't': round(now), 'h': sample['height'], 'node': sample['node'], 'cpu': sample['cpu_percent'],
        'mem': sample['memory_percent'], 'disk_free': sample['disk_free_percent'], 'disk_used': sample['disk_used'],
        'rx': sample['rx_rate'], 'tx': sample['tx_rate'], 'peers': sample['peers'], 'mempool': sample['mempool'],
        'missed': sample['missed'], 'sig_invalid': sample['signature_invalid'], 'staked': sample['staked'],
        'emitted': sample['emitted'], 'burned': sample['burned'], 'restarts': len(state['starts']),
        'rpc': sample['rpc_status'], 'rpc_ms': sample['rpc_ms'], 'firing': firing}, now)
    # After the heartbeat, so a slow store never delays it; the next
    # heartbeat carries what still waits.
    state['history_pending'] = upload_history(config, state, state_dir, now, run, errors)
    save_state(state_dir, state)
    summary = {'status': 'OK' if not errors else 'PARTIAL', 'firing': firing, 'delivered': delivered,
               'heartbeat': 'SENT' if beat else ('NO_SETTINGS' if settings is None else 'FAILED'),
               'undelivered': len(state['outbox']), 'history_pending': state['history_pending'], 'errors': errors}
    out(json.dumps(summary))
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--config', type=Path, required=True)
    args = parser.parse_args()
    try:
        run_once(args.config)
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'monitor: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
