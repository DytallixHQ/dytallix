#!/usr/bin/env python3
"""Write or check the release manifest, the supervisor's catalog (E06).

  release_manifest.py write --record BUILD_RECORD.json --native-genesis native-genesis.json
                            --out RELEASE_MANIFEST.json [--chain-id ID]
  release_manifest.py check --manifest RELEASE_MANIFEST.json --record BUILD_RECORD.json
                            --native-genesis native-genesis.json [--chain-id ID]

The manifest is release-runtime's ManifestV2 (component_candidate.rs) for
the production service profile, in serde's canonical compact encoding. It
names the chain, the native genesis digest and the compiled migration
registry, and lists each catalog role's binary from the build record. Every
member is a static executable, so no runtime profile names a library (P01,
5 October 2026).

Its SHA-512 is the release's authority once the 3-of-5 genesis keys sign it
(P01, 5 October 2026): it goes into GENESIS_INPUTS.json as
`root.release_sha512`, which the genesis builder writes into the
application configuration and the root genesis binds. The native genesis
does not depend on it, so the order at the final freeze is:

  1. build the genesis and take native-genesis.json;
  2. write the manifest from it and the release's BUILD_RECORD.json;
  3. set root.release_sha512 and build the genesis again; its
     native-genesis.json is unchanged.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

SCHEMA = 2
SERVICE_PROFILE = 'production-linux-native-service-v1'
MAPPING_POLICY = 'linux-observed-code-v1'
RECORD_SCHEMA = 'dytallix.release-build-record.v1'
TARGET = {'os': 'linux', 'arch': 'x86_64', 'abi': 'gnu'}
# The production profile's roles (component_candidate.rs), all native.
ROLES = ('consensus_bridge', 'consensus_engine', 'consensus_stdio', 'control_verifier',
         'genesis_bootstrap_verifier', 'http_adapter', 'service_supervisor')
HERE = Path(__file__).resolve().parent
IDENTITY = HERE.parent/'launch'/'genesis'/'IDENTITY.json'
IDENTIFIER = re.compile(r'^[A-Za-z0-9_.-]{1,128}$')
MAX_MANIFEST_BYTES = 1024 * 1024


def encode(manifest):
    """serde_json's encoding of ManifestV2: struct field order, compact."""
    return json.dumps(manifest, separators=(',', ':'), ensure_ascii=False).encode()


def hex_digest(value, size, label):
    if not (isinstance(value, str) and re.fullmatch(f'[0-9a-f]{{{size}}}', value)):
        raise ValueError(f'{label} must be {size} lowercase hex digits')
    return value


def load_record(path):
    record = json.loads(Path(path).read_text())
    if record.get('schema') != RECORD_SCHEMA: raise ValueError('build record schema mismatch')
    if record['target'] != TARGET: raise ValueError('the release targets Linux x86_64 GNU')
    if record.get('linkage') != 'static': raise ValueError('release members must be static executables')
    hex_digest(record.get('migration_registry_sha256'), 64, 'migration_registry_sha256')
    if set(record['catalog_roles']) != set(ROLES):
        raise ValueError('the build record must name every production role')
    for role, name in sorted(record['catalog_roles'].items()):
        member = record['members'].get(name)
        if member is None: raise ValueError(f'{role}: {name} is not in the build record')
    return record


def manifest(record, chain_id, native_genesis):
    """The ManifestV2 value, fields in struct order and arrays sorted."""
    if not IDENTIFIER.fullmatch(chain_id): raise ValueError('invalid chain id')
    names = sorted(set(record['catalog_roles'].values()))
    members = [{'id': name, 'kind': 'executable', 'bytes': record['members'][name]['bytes'],
                'sha256': hex_digest(record['members'][name]['sha256'], 64, name),
                'sha512': hex_digest(record['members'][name]['sha512'], 128, name)} for name in names]
    contents = {(m['bytes'], m['sha256'], m['sha512']) for m in members}
    if len(contents) != len(members): raise ValueError('two catalog members have the same content')
    return {
        'schema': SCHEMA,
        'chain_id': chain_id,
        'app_genesis_sha256': hashlib.sha256(Path(native_genesis).read_bytes()).hexdigest(),
        'target': {'os': TARGET['os'], 'arch': TARGET['arch'], 'abi': TARGET['abi']},
        'migration_registry_sha256': record['migration_registry_sha256'],
        'service_profile': SERVICE_PROFILE,
        'members': members,
        'roles': [{'role': role, 'member_id': record['catalog_roles'][role], 'runtime_profile_id': role}
                  for role in ROLES],
        'runtime_profiles': [{'id': role, 'member_ids': [], 'mapping_policy': MAPPING_POLICY,
                              'interpreted_member_ids': []} for role in ROLES],
    }


def summary(raw, value):
    verifier = next(m for m in value['members']
                    if m['id'] == next(r['member_id'] for r in value['roles'] if r['role'] == 'genesis_bootstrap_verifier'))
    return {
        'release_sha512': hashlib.sha512(raw).hexdigest(),
        'bytes': len(raw),
        'chain_id': value['chain_id'],
        'app_genesis_sha256': value['app_genesis_sha256'],
        'migration_registry_sha256': value['migration_registry_sha256'],
        # The root helper's pin in each node's root bootstrap configuration.
        'bootstrap_verifier': {k: verifier[k] for k in ('bytes', 'sha256', 'sha512')},
    }


def default_chain_id():
    return json.loads(IDENTITY.read_text())['chain_id']


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    for name in ('write', 'check'):
        c = commands.add_parser(name)
        c.add_argument('--record', type=Path, required=True, help="the release's BUILD_RECORD.json")
        c.add_argument('--native-genesis', type=Path, required=True, help="the genesis builder's native-genesis.json")
        c.add_argument('--chain-id', help='default: launch/genesis/IDENTITY.json')
        if name == 'write':
            c.add_argument('--out', type=Path, required=True)
        else:
            c.add_argument('--manifest', type=Path, required=True)
    args = parser.parse_args()
    value = manifest(load_record(args.record), args.chain_id or default_chain_id(), args.native_genesis)
    raw = encode(value)
    if len(raw) > MAX_MANIFEST_BYTES: raise ValueError('manifest exceeds 1 MiB')
    if args.command == 'write':
        if args.out.exists(): raise ValueError(f'{args.out} already exists')
        args.out.write_bytes(raw)
        print(json.dumps(dict(summary(raw, value), status='WRITTEN'), indent=2))
        return 0
    same = args.manifest.read_bytes() == raw
    print(json.dumps(dict(summary(raw, value), status='MATCHES' if same else 'DIFFERS'), indent=2))
    return 0 if same else 1


if __name__ == '__main__': raise SystemExit(main())
