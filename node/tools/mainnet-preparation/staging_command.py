#!/usr/bin/env python3
"""Make and sign the staging agent's commands (simulation plan, T2;
node/docs/architecture/staging-agent-v1.md), on the founder's machine, with
the release's own CLI: the staging command key is an ML-DSA-65 key in a
keystore of its own, made by `dytallix wallet create` and used by
`dytallix crypto sign`. Staging only; never a production key.

  staging_command.py key --cli DYTALLIX --home DIR --out KEY.json
  staging_command.py settings --key KEY.json --label L --chain ID --store READ.json --out FILE
  staging_command.py make --cli DYTALLIX --home DIR --key KEY.json --chain ID --host LABEL
                          --sequence N --command NAME [--arg NAME=INT ...] [--valid-minutes M] --out FILE
  staging_command.py package --out DIR

key makes the staging command key in a new keystore under DIR (the CLI asks
for its passphrase, or reads DYTALLIX_KEYSTORE_PASSPHRASE_FILE) and writes
its public record. settings writes one host's agent settings: its label,
the rehearsal chain, the pinned public key and the command store's read key.
make checks the command against the allowlist, signs it, checks the
signature with the CLI and writes the command file; upload it to the
command store under the key it prints (the store's prefix first). package
writes the agent's archive for a host and its SHA-256 for the console.
Results are opened with `dytallix-root-sign history-open` and the chain's
history code.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import time

HERE = Path(__file__).resolve().parent
AGENT_DIR = HERE / 'staging-agent'
sys.path.insert(0, str(AGENT_DIR))
import agent  # noqa: E402

KEY_SCHEMA = 'dytallix.staging-command-key.v1'
WALLET = 'staging-command'
SIGNATURE = re.compile(r'^[0-9a-f]{%d}$' % agent.SIGNATURE_HEX, re.M)
ARCHIVE = 'staging-agent.tar'


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def write_new(path, raw, mode=0o644):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(descriptor, 'wb') as file:
        file.write(raw)


def cli_env(home):
    return dict(os.environ, HOME=str(home))


def keystore_entry(home):
    """The staging command key's public key from its keystore, which keeps
    public keys in the clear; it must be the active wallet."""
    keystore = json.loads((Path(home) / '.dytallix' / 'keystore.json').read_bytes())
    require(keystore.get('active') == WALLET, f'the keystore\'s active wallet is not {WALLET}')
    entry = next((e for e in keystore['entries'] if e['name'] == WALLET), None)
    require(entry is not None, f'the keystore has no {WALLET} wallet')
    raw = bytes(entry['public_key'])
    require(len(raw) == agent.PUBLIC_KEY_HEX // 2, f'{WALLET} is not an ML-DSA-65 key')
    return raw.hex()


def load_key(path):
    record = json.loads(Path(path).read_bytes())
    require(record.get('schema') == KEY_SCHEMA, f'{path} is not a staging command key ({KEY_SCHEMA})')
    key = record['public_key_hex']
    require(len(key) == agent.PUBLIC_KEY_HEX and agent.HEX.match(key), f'{path} holds no ML-DSA-65 public key')
    require(record['key_id'] == hashlib.sha256(bytes.fromhex(key)).hexdigest(), f'{path}: the key ID does not match')
    return key


def make_key(cli, home, out, run=subprocess.run):
    home = Path(home)
    home.mkdir(mode=0o700)
    result = run([str(cli), 'wallet', 'create', '--name', WALLET], env=cli_env(home))
    require(result.returncode == 0, 'dytallix wallet create failed')
    key = keystore_entry(home)
    record = {'schema': KEY_SCHEMA, 'public_key_hex': key, 'key_id': hashlib.sha256(bytes.fromhex(key)).hexdigest()}
    write_new(out, json.dumps(record, indent=1).encode())
    print(f'staging command key {record["key_id"][:16]} in {home}; public record {out}')
    return record


def make_settings(key_path, label, chain, store_path, out):
    store = json.loads(Path(store_path).read_bytes())
    settings = agent.check_settings({
        'schema': agent.SETTINGS_SCHEMA, 'label': label, 'chain_id': chain, 'command_public_key_hex': load_key(key_path),
        'commands': {field: store[field] for field in agent.STORE_FIELDS}})
    write_new(out, json.dumps(settings, indent=1).encode(), 0o600)
    print(f'agent settings for {label} on {chain}: {out} (it holds the read key; keep it on your own media)')
    return settings


def body(chain, host, sequence, command, args, not_after):
    return {'schema': agent.COMMAND_SCHEMA, 'chain_id': chain, 'host': host, 'sequence': sequence,
            'not_after': not_after, 'command': command, 'args': args}


def check_allowed(command, args, allowlist=None):
    """The agent's own allowlist check, before anything is signed."""
    allowlist = allowlist or agent.check_allowlist(json.loads((AGENT_DIR / 'ALLOWLIST.json').read_bytes()))
    entry = allowlist['commands'].get(command)
    require(entry is not None, f'{command} is not on the allowlist: {", ".join(sorted(allowlist["commands"]))}')
    require(set(args) == set(entry['args']), f'{command} takes {sorted(entry["args"])}')
    for name, value in args.items():
        spec = entry['args'][name]
        require(spec['min'] <= value <= spec['max'], f'{name} is outside {spec["min"]} to {spec["max"]}')


def sign(cli, home, key, unsigned, run=subprocess.run):
    """Signs a body with the staging command key and checks the signature
    with the CLI before returning the command file's bytes."""
    require(keystore_entry(home) == key, 'the keystore\'s staging command key is not the key record\'s')
    text = agent.message(unsigned)
    result = run([str(cli), 'crypto', 'sign', text], env=cli_env(home), capture_output=True, text=True)
    require(result.returncode == 0, f'dytallix crypto sign failed: {(result.stderr or result.stdout).strip()}')
    found = SIGNATURE.findall(result.stdout)
    require(len(found) == 1, 'dytallix crypto sign printed no ML-DSA-65 signature')
    checked = run([str(cli), 'crypto', 'verify', text, found[0], key], env=cli_env(home), capture_output=True, text=True)
    require(checked.returncode == 0, 'the new signature does not verify')
    return json.dumps({'body': unsigned, 'signature': found[0]}, indent=1).encode()


def object_key(unsigned):
    return f'{unsigned["chain_id"]}/{unsigned["host"]}/{unsigned["sequence"]:010}.json'


def parse_args(pairs):
    args = {}
    for pair in pairs or []:
        name, _, value = pair.partition('=')
        require(re.fullmatch(r'[a-z_]{1,32}', name) and re.fullmatch(r'-?[0-9]{1,12}', value) and name not in args,
                f'--arg {pair}: NAME=INTEGER, each name once')
        args[name] = int(value)
    return args


def make(cli, home, key_path, chain, host, sequence, command, args, valid_minutes, out, clock=time.time,
         run=subprocess.run):
    require(1 <= valid_minutes <= agent.MAX_VALIDITY_SECONDS // 60, 'a command is valid for 1 minute to 24 hours')
    check_allowed(command, args)
    unsigned = body(chain, host, sequence, command, args, int(clock()) + 60 * valid_minutes)
    raw = sign(cli, home, load_key(key_path), unsigned, run)
    write_new(out, raw)
    print(f'signed {command} for {host} on {chain}, sequence {sequence}: {out}\n'
          f'upload it to the command store as PREFIX{object_key(unsigned)}')
    return raw


def package(out):
    """A deterministic archive of the agent and its allowlist."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w', format=tarfile.PAX_FORMAT) as archive:
        for name, mode in (('agent.py', 0o555), ('ALLOWLIST.json', 0o444)):
            raw = (AGENT_DIR / name).read_bytes()
            info = tarfile.TarInfo(f'dytallix-agent/{name}')
            info.size, info.mode, info.mtime, info.uid, info.gid, info.uname, info.gname = len(raw), mode, 0, 0, 0, 'root', 'root'
            archive.addfile(info, io.BytesIO(raw))
    raw = buffer.getvalue()
    Path(out).mkdir(parents=True, exist_ok=True)
    write_new(Path(out) / ARCHIVE, raw)
    digest = hashlib.sha256(raw).hexdigest()
    write_new(Path(out) / f'{ARCHIVE}.sha256', f'{digest}  {ARCHIVE}\n'.encode())
    print(f'{Path(out) / ARCHIVE} sha256 {digest}')
    return digest


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    k = commands.add_parser('key')
    k.add_argument('--cli', type=Path, required=True)
    k.add_argument('--home', type=Path, required=True)
    k.add_argument('--out', type=Path, required=True)
    s = commands.add_parser('settings')
    s.add_argument('--key', type=Path, required=True)
    s.add_argument('--label', required=True)
    s.add_argument('--chain', required=True)
    s.add_argument('--store', type=Path, required=True)
    s.add_argument('--out', type=Path, required=True)
    m = commands.add_parser('make')
    m.add_argument('--cli', type=Path, required=True)
    m.add_argument('--home', type=Path, required=True)
    m.add_argument('--key', type=Path, required=True)
    m.add_argument('--chain', required=True)
    m.add_argument('--host', required=True)
    m.add_argument('--sequence', type=int, required=True)
    m.add_argument('--command', dest='name', required=True)
    m.add_argument('--arg', action='append')
    m.add_argument('--valid-minutes', type=int, default=60)
    m.add_argument('--out', type=Path, required=True)
    p = commands.add_parser('package')
    p.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'key':
            make_key(args.cli, args.home, args.out)
        elif args.command == 'settings':
            make_settings(args.key, args.label, args.chain, args.store, args.out)
        elif args.command == 'make':
            make(args.cli, args.home, args.key, args.chain, args.host, args.sequence, args.name, parse_args(args.arg),
                 args.valid_minutes, args.out)
        else:
            package(args.out)
    except (Failed, agent.Failed, OSError, KeyError, ValueError) as error:
        print(f'staging_command: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
