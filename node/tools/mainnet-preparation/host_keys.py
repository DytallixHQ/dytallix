#!/usr/bin/env python3
"""Make every host's node keys offline and seal them (E05; host setup v1,
node/docs/architecture/host-setup-v1.md). Runs on the ceremony machine.

  host_keys.py --plan PIN_PLAN.json --bin DIR --staging DIR --out DIR [--backup-upload FILE]

For each host in the pin plan, in a new staging node home STAGING/LABEL:
  - the peer seed (dytallix-peer-seed generate);
  - the validator key and its fresh signing state (dytallix-validator-key
    generate);
  - on the endpoint, the client channel seed and its public pin
    (dytallix-channel-key generate and pin, for the plan's channel address).
Then `dytallix-root-sign seal` seals the host's secret files under a new
seal code and prints its paper line. Write it on paper twice; the founder
types it back from the paper and `seal-check` confirms it opens the keys.

With --backup-upload (disaster recovery v1), the sentry also gets the
chain's backup code (`dytallix-root-sign backup-code`; its paper line is
written twice and typed back) and the off-host store's write-only upload
key, FILE (dytallix.backup-upload.v1, owner-only: endpoint, bucket, region,
prefix, access_key_id, secret_access_key); both are sealed with its keys.

OUT receives only public files: LABEL.keys.json (dytallix.host-keys.v1: the
public keys and each secret file's SHA-256), LABEL.sealed.json, the
endpoint's LABEL.channel-pin.json, and PIN_PLAN.json, the plan with the
public keys filled in. --bin holds the release binaries. Keep STAGING in
memory (the live session's /tmp) and power the machine off afterwards; the
sealed records are the keys' backup.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

KEYS_SCHEMA = 'dytallix.host-keys.v1'
PLAN_SCHEMA = 'dytallix.pin-plan.v1'
SECRETS = ('config/pqc_peer_seed.bin', 'config/priv_validator_key.json', 'data/priv_validator_state.json')
CHANNEL_SEED = 'config/client_channel_seed.bin'
BACKUP_SECRETS = ('backup/code', 'backup/upload.json')
UPLOAD_SCHEMA = 'dytallix.backup-upload.v1'
UPLOAD_FIELDS = ('schema', 'endpoint', 'bucket', 'region', 'prefix', 'access_key_id', 'secret_access_key')


class Invalid(ValueError):
    pass


def require(ok, message):
    if not ok:
        raise Invalid(message)


def tool(bin_dir, name, *args, interactive=False):
    """Runs a release binary; returns its output, parsed when it is JSON."""
    command = [str(Path(bin_dir) / name), *args]
    if interactive:
        result = subprocess.run(command)
        require(result.returncode == 0, f'{name} {args[0]} failed')
        return None
    result = subprocess.run(command, capture_output=True, text=True)
    require(result.returncode == 0, f'{name} {args[0]} failed: {result.stderr.strip()}')
    try:
        return json.loads(result.stdout)
    except ValueError:
        return result.stdout


def read_upload(path):
    """The write-only upload key, owner-only, with exactly its fields."""
    require(os.stat(path).st_mode & 0o077 == 0, f'{path} must be readable only by its owner')
    upload = json.loads(Path(path).read_bytes())
    require(type(upload) is dict and sorted(upload) == sorted(UPLOAD_FIELDS) and upload['schema'] == UPLOAD_SCHEMA
            and all(type(upload[f]) is str for f in UPLOAD_FIELDS), f'{path} is not a backup upload key')
    require(upload['endpoint'].startswith('https://') or upload['endpoint'].startswith('http://127.0.0.1'),
            'the upload endpoint must use https')
    return upload


def code_line(text):
    return ' '.join(text.split()).lower()


def backup_secrets(plan, home, bin_dir, upload, show, read_line):
    """The sentry's backup code and upload key, in its staging home."""
    directory = home / 'backup'
    directory.mkdir(mode=0o700)
    os.chmod(directory, 0o700)
    descriptor = os.open(directory / 'upload.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as file:
        file.write(json.dumps(upload, indent=2) + '\n')
    printed = tool(bin_dir, 'dytallix-root-sign', 'backup-code', '-chain', plan['chain_id'],
                   '-out', str(directory / 'code'))
    show(printed if isinstance(printed, str) else json.dumps(printed))
    expected = code_line((directory / 'code').read_text())
    for attempt in range(3):
        show(f'Write the backup code for {plan["chain_id"]} on paper twice. Then type it back from the paper '
             'and press Enter:')
        if code_line(read_line()) == expected:
            return
        show('That is not the printed backup code; check every group.')
    raise Invalid('the backup code was not typed back correctly')


def host_keys(plan, plan_host, bin_dir, staging, out, show=print, upload=None, read_line=sys.stdin.readline):
    label, role = plan_host['label'], plan_host['role']
    home = Path(staging) / label
    require(not home.exists(), f'{home} exists; use a new staging directory')
    for directory in (home, home / 'config', home / 'data'):
        directory.mkdir(mode=0o700)
        os.chmod(directory, 0o700)
    peer = tool(bin_dir, 'dytallix-peer-seed', 'generate', '--home', str(home))
    validator = tool(bin_dir, 'dytallix-validator-key', 'generate',
                     '--key-file', str(home / 'config' / 'priv_validator_key.json'),
                     '--state-file', str(home / 'data' / 'priv_validator_state.json'))
    secrets = list(SECRETS)
    if role == 'endpoint':
        require(plan_host.get('channel'), f'{label}: the endpoint needs its channel address in the plan')
        tool(bin_dir, 'dytallix-channel-key', 'generate', '--seed-file', str(home / CHANNEL_SEED))
        tool(bin_dir, 'dytallix-channel-key', 'pin', '--seed-file', str(home / CHANNEL_SEED),
             '--network', plan['chain_id'], '--address', plan_host['channel'],
             '--output', str(Path(out) / f'{label}.channel-pin.json'))
        secrets.append(CHANNEL_SEED)
    if role == 'sentry' and upload is not None:
        backup_secrets(plan, home, bin_dir, upload, show, read_line)
        secrets.extend(BACKUP_SECRETS)
    secrets.sort()
    summary = {
        'schema': KEYS_SCHEMA, 'label': label, 'role': role,
        'peer_public_key_base64': peer['public_key_base64'],
        'validator_public_key_base64': validator['public_key_base64'],
        'secret_files': {path: hashlib.sha256((home / path).read_bytes()).hexdigest() for path in secrets},
    }
    with open(Path(out) / f'{label}.keys.json', 'x') as file:
        file.write(json.dumps(summary, indent=2) + '\n')
    sealed = Path(out) / f'{label}.sealed.json'
    printed = tool(bin_dir, 'dytallix-root-sign', 'seal', '-label', label, '-home', str(home), '-out', str(sealed),
                   *secrets)
    show(printed if isinstance(printed, str) else json.dumps(printed))
    show(f'Write the seal code for {label} on paper twice. Then type it back from the paper and press Enter:')
    tool(bin_dir, 'dytallix-root-sign', 'seal-check', '-paper', '-', '-sealed', str(sealed), interactive=True)
    return summary


def run(plan_path, bin_dir, staging, out, show=print, backup_upload=None, read_line=sys.stdin.readline):
    plan = json.loads(Path(plan_path).read_bytes())
    require(plan.get('schema') == PLAN_SCHEMA, 'not a pin plan')
    upload = read_upload(backup_upload) if backup_upload else None
    require(upload is None or any(h['role'] == 'sentry' for h in plan['hosts']), 'backups need a sentry in the plan')
    Path(out).mkdir(mode=0o755, exist_ok=False)
    Path(staging).mkdir(mode=0o700, exist_ok=True)
    summaries = {}
    for plan_host in plan['hosts']:
        show(f'== {plan_host["label"]} ({plan_host["role"]})')
        summaries[plan_host['label']] = host_keys(plan, plan_host, bin_dir, staging, out, show, upload, read_line)
    filled = json.loads(json.dumps(plan))
    for plan_host in filled['hosts']:
        summary = summaries[plan_host['label']]
        plan_host['peer_public_key_base64'] = summary['peer_public_key_base64']
        plan_host['validator_public_key_base64'] = summary['validator_public_key_base64']
    with open(Path(out) / 'PIN_PLAN.json', 'x') as file:
        file.write(json.dumps(filled, indent=2) + '\n')
    return summaries


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for name in ('plan', 'bin', 'staging', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--backup-upload', type=Path, help="the off-host store's write-only upload key")
    args = parser.parse_args()
    try:
        summaries = run(args.plan, args.bin, args.staging, args.out, backup_upload=args.backup_upload)
    except (Invalid, OSError, KeyError, ValueError) as error:
        print(f'host_keys: {error}', file=sys.stderr)
        return 2
    print(json.dumps({'status': 'SEALED', 'hosts': sorted(summaries), 'out': str(args.out)}, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
