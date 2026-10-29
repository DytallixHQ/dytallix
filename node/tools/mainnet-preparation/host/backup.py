#!/usr/bin/env python3
"""Copy the sentry's newest snapshot off the host (E05; disaster recovery v1,
node/docs/architecture/disaster-recovery-v1.md). The dytallix-backup unit
runs it hourly from its timer, as root with only CAP_DAC_READ_SEARCH:

  backup.py --config /etc/dytallix/RELEASE/backup.json

It takes the newest height with both a published snapshot
(SNAPSHOTS/{height:020} with its metadata.json) and the light blocks the
engine wrote for it (LIGHT_BLOCKS/{height:020}, heights H to H+2); if that
height is above the last one uploaded, it encrypts them together with the
release's `dytallix-root-sign backup-seal` under the chain's backup code, uploads the copy with the host's curl (AWS Signature V4, to an
S3-compatible bucket at a second provider, with a write-only key), records
the height and removes its scratch copy. The upload key reaches curl on its
standard input, never on a command line. Standard library only.

The copy holds every block record since genesis, and a store takes at most
5 GB in one upload (Backblaze B2), so it goes up in parts of 1 GiB
(P01, 28 October 2026): PREFIX/CHAIN/{height:020}-{sha256}/part-NNNN.bin,
then manifest.json (dytallix.backup-parts.v1: each part's size and SHA-256,
the copy's size and SHA-256), last, so a copy is complete only once its
manifest exists. The scratch directory holds the copy and one part.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import urlsplit

CONFIG_SCHEMA = 'dytallix.host-backup.v1'
UPLOAD_SCHEMA = 'dytallix.backup-upload.v1'
HEIGHT_DIR = re.compile(r'^[0-9]{20}$')
# Values that go into curl's configuration unquoted-safe.
SAFE = re.compile(r'^[A-Za-z0-9._~+/=-]{1,256}$')
STATE_FILE = 'last-uploaded'
PARTS_SCHEMA = 'dytallix.backup-parts.v1'
PART_BYTES = 1 << 30
MANIFEST = 'manifest.json'


class Failed(Exception):
    pass


def require(ok, message):
    if not ok:
        raise Failed(message)


def read_json(path, limit=65536):
    raw = Path(path).read_bytes()
    require(len(raw) <= limit, f'{path} is too large')
    return json.loads(raw)


def load(config_path):
    config = read_json(config_path)
    require(config.get('schema') == CONFIG_SCHEMA, 'not a host backup configuration')
    upload = read_json(config['upload_file'])
    require(upload.get('schema') == UPLOAD_SCHEMA, 'not a backup upload key')
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
    return config, upload


def newest_snapshot(snapshots, light_blocks):
    """The newest height with a published snapshot and its light blocks."""
    published = {entry.name for entry in Path(light_blocks).iterdir() if HEIGHT_DIR.match(entry.name) and entry.is_dir()}
    heights = sorted(int(entry.name) for entry in Path(snapshots).iterdir()
                     if HEIGHT_DIR.match(entry.name) and (entry / 'metadata.json').is_file() and entry.name in published)
    return heights[-1] if heights else None


def last_uploaded(state):
    path = Path(state) / STATE_FILE
    return int(path.read_text().strip()) if path.exists() else 0


def record(state, height):
    path = Path(state) / STATE_FILE
    temporary = path.with_name(STATE_FILE + '.new')
    temporary.write_text(f'{height}\n')
    os.replace(temporary, path)


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, 'rb') as file:
        for block in iter(lambda: file.read(1 << 20), b''):
            digest.update(block)
    return digest.hexdigest()


def curl_config(upload, copy, url):
    """curl's options, read from its standard input."""
    return '\n'.join([
        'fail', 'silent', 'show-error',
        'max-time = 3600', 'retry = 3',
        f'aws-sigv4 = "aws:amz:{upload["region"]}:s3"',
        f'user = "{upload["access_key_id"]}:{upload["secret_access_key"]}"',
        f'upload-file = "{copy}"',
        f'url = "{url}"',
        '']).encode()


def put(config, upload, runner, path, name):
    url = f'{upload["endpoint"].rstrip("/")}/{upload["bucket"]}/{name}'
    result = runner([config['curl'], '--config', '-'], input=curl_config(upload, path, url), capture_output=True)
    require(result.returncode == 0,
            f'the upload failed ({name}): {(result.stderr or b"").decode(errors="replace").strip()}')


def split(copy, part, part_bytes):
    """Yields (index, bytes, sha256) as each part of the copy is written to
    the one part file."""
    with open(copy, 'rb') as source:
        for index in range(max(1, -(-copy.stat().st_size // part_bytes))):
            digest, size = hashlib.sha256(), 0
            with open(part, 'wb') as target:
                while size < part_bytes:
                    block = source.read(min(1 << 20, part_bytes - size))
                    if not block:
                        break
                    digest.update(block)
                    target.write(block)
                    size += len(block)
            yield index, size, digest.hexdigest()


def run(config_path, runner=subprocess.run, out=print, part_bytes=PART_BYTES):
    config, upload = load(config_path)
    chain, snapshots, state = config['chain_id'], config['snapshots'], config['state']
    height = newest_snapshot(snapshots, config['light_blocks'])
    if height is None or height <= last_uploaded(state):
        out(json.dumps({'status': 'NOTHING_NEW', 'newest': height, 'uploaded': last_uploaded(state)}))
        return None
    scratch = Path(state) / 'scratch'
    copy = scratch / f'{chain}-{height:020}.bin'
    part, manifest_file = scratch / 'part.bin', scratch / MANIFEST
    for leftover in (copy, copy.with_name(copy.name + '.partial'), part, manifest_file):
        if leftover.exists():
            leftover.unlink()
    try:
        result = runner([config['signer'], 'backup-seal', '-code-file', config['code_file'], '-height', str(height),
                         '-in', str(Path(snapshots) / f'{height:020}'),
                         '-light-blocks', str(Path(config['light_blocks']) / f'{height:020}'), '-out', str(copy)],
                        capture_output=True, text=True)
        require(result.returncode == 0, f'backup-seal failed: {(result.stderr or "").strip()}')
        digest, size = sha256_file(copy), copy.stat().st_size
        folder = f'{upload["prefix"]}{chain}/{height:020}-{digest}/'
        parts = []
        for index, part_size, part_digest in split(copy, part, part_bytes):
            name = f'part-{index:04}.bin'
            put(config, upload, runner, part, folder + name)
            parts.append({'name': name, 'bytes': part_size, 'sha256': part_digest})
        manifest_file.write_text(json.dumps({
            'schema': PARTS_SCHEMA, 'chain_id': chain, 'height': height, 'copy_bytes': size, 'copy_sha256': digest,
            'part_bytes': part_bytes, 'parts': parts}, indent=2) + '\n')
        # Last: a copy without its manifest is incomplete.
        put(config, upload, runner, manifest_file, folder + MANIFEST)
        record(state, height)
    finally:
        for leftover in (copy, part, manifest_file):
            if leftover.exists():
                leftover.unlink()
    summary = {'status': 'UPLOADED', 'chain_id': chain, 'height': height, 'copy': folder, 'sha256': digest,
               'bytes': size, 'parts': len(parts)}
    out(json.dumps(summary))
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--config', type=Path, required=True)
    args = parser.parse_args()
    try:
        run(args.config)
    except (Failed, OSError, KeyError, ValueError) as error:
        print(f'backup: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
