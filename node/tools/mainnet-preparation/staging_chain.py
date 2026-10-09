#!/usr/bin/env python3
"""Build a throwaway production-profile staging chain and its host bundles
(E05; host setup v1, step H4: node/docs/architecture/host-setup-v1.md).

  staging_chain.py --plan PIN_PLAN.json --keys KEYS_DIR --release RELEASE_DIR \
                   --tools TOOLS_DIR --out OUT_DIR [--genesis-time TIME]

Inputs:
  --plan     the pin plan with public keys filled in (host_keys.py writes it);
             a staging chain ID, never the approved network identity
  --keys     host_keys.py's output: each host's key summary, sealed keys and
             the endpoint's channel pin
  --release  a release build (BUILD_RECORD.json and bin/)
  --tools    production builds of dytallix-genesis-build and
             dytallix-host-config, and dytallix-root-sign

It runs the launch pipeline end to end with synthetic records, in the
release manifest's freeze order:
  1. records: the committed production rehearsal's synthetic records, with
     this plan's validators (their keys from the key step), chain ID and
     genesis time;
  2. resolve and build the genesis (production builder);
  3. write the release manifest from the build record and native genesis;
  4. bind root.release_sha512 and build again (the native genesis must not
     change);
  5. make five throwaway root genesis keys and sign three of five;
     (before step 1, five throwaway key kits replace the rehearsal's
     freeze, resume and upgrade keys, which nothing can sign for, so that
     the kit replacement drill can sign controls; the upgrade and handover
     notice is a staging-only 20 blocks: P01, 8 October 2026)
  6. resolve the host values and run dytallix-host-config;
  7. generate every host's files (host_files.py) and bundle them
     (host_bundle.py).

OUT_DIR receives each stage's files and bundles/LABEL.bundle.tar. Its
root-keys/ and control-kits/ hold the throwaway private keys: this chain is
for staging and CI only, its records and keys are invented, and nothing here
may be reused for any network.
"""
import argparse
import copy
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
NODE = HERE.parents[1]
MAINNET = HERE.parents[2]
sys.path.insert(0, str(MAINNET / 'release'))
import host_bundle  # noqa: E402
import host_files  # noqa: E402
import release_manifest  # noqa: E402

REHEARSAL = HERE / 'fixtures' / 'genesis-production-rehearsal'
IDENTITY = MAINNET / 'launch' / 'genesis' / 'IDENTITY.json'
E05_VALUES = MAINNET / 'launch' / 'E05_VALUES.json'
GENESIS_PROPOSALS = MAINNET / 'launch' / 'genesis' / 'PROPOSALS.json'
HOST_PROPOSALS = MAINNET / 'launch' / 'hosts' / 'PROPOSALS.json'
SETUP_VALUES = MAINNET / 'launch' / 'hosts' / 'SETUP_VALUES.json'
ROOT_KEYS = 5
ROOT_SIGNERS = (1, 3, 5)
CONTROL_KITS = 5
# Staging only (P01, 8 October 2026, the kit replacement drill): a CI run
# cannot wait 120,960 blocks for a kit replacement to take effect.
STAGING_NOTICE_BLOCKS = 20
NOTICE_PATHS = ('upgrade.min_notice_blocks', 'release_handover.v2.min_notice_blocks')
DRILL_APPROVAL = 'launch/approvals/P01_E05_KIT_REPLACEMENT_DRILL_2026-10-08.json'
BOUNDARY = ('Staging chain for host setup v1 (H4): the production rehearsal\'s invented records with '
            'throwaway validator and root keys. Not for any network.')


class Invalid(ValueError):
    pass


def require(ok, message):
    if not ok:
        raise Invalid(message)


def read_json(path):
    return json.loads(Path(path).read_bytes())


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2) + '\n')


def run(args, show, capture=True):
    show('$ ' + ' '.join(str(a) for a in args))
    result = subprocess.run([str(a) for a in args], capture_output=capture, text=True)
    if capture and result.stdout.strip():
        show(result.stdout.rstrip())
    if result.returncode != 0:
        raise Invalid(f'{Path(str(args[0])).name} failed: {(result.stderr or "").strip()}')
    return result.stdout if capture else ''


def staging_records(plan, rehearsal, genesis_time):
    """The rehearsal's synthetic records with the plan's validators."""
    identity = read_json(IDENTITY)
    chain = plan['chain_id']
    require(chain != identity['chain_id'] and 'mainnet' not in chain and 'production' not in chain,
            f'{chain} is not a staging chain ID')
    validators = [h for h in plan['hosts'] if h['role'] == 'validator']
    require(1 <= len(validators) <= len(rehearsal['validators']),
            f'a staging plan has 1 to {len(rehearsal["validators"])} validators')
    records = copy.deepcopy(rehearsal)
    records['boundary'] = BOUNDARY
    records['chain_id'] = chain
    records['genesis_time'] = genesis_time
    # Each validator takes a rehearsal operator's place and self-bond.
    records['validators'] = [dict(template, validator_id=host['label'],
                                  consensus_public_key_base64=host['validator_public_key_base64'])
                             for template, host in zip(rehearsal['validators'], validators)]
    kept = {row['validator_id'] for row in records['validators']}
    records['delegations'] = [d for d in rehearsal['delegations'] if d['validator_id'] in kept]
    return records


def control_kits(tools, out, show):
    """Five throwaway key kits for the root controls, made as the key
    ceremony makes them; their freeze, resume and upgrade keys replace the
    rehearsal's. Kit N's private keys are in control-kits/kit-N, its public
    key records in control-kits/public. Returns the three authorities."""
    kits = out / 'control-kits'
    kits.mkdir(mode=0o700)
    public = kits / 'public'
    public.mkdir()
    for n in range(1, CONTROL_KITS + 1):
        drive = kits / f'kit-{n}'
        drive.mkdir(mode=0o700)
        # The kit's paper line is not shown: the drive holds it.
        run([tools / 'dytallix-root-sign', 'kit', '-number', n, '-private-out', drive, '-public-out', public],
            lambda text: show('\n'.join(line for line in text.splitlines() if not line.startswith('dytallix-kit-'))))

    def authority(purpose):
        keys = [read_json(public / f'kit-{n}-{purpose}.json') for n in range(1, CONTROL_KITS + 1)]
        return {'keys': sorted(keys, key=lambda key: key['key_id']), 'threshold': 3}
    return {purpose: authority(purpose) for purpose in ('freeze', 'resume', 'upgrade')}


def with_control_kits(records, authorities):
    """The records with the kits' authorities for the root controls."""
    records = copy.deepcopy(records)
    records['root']['emergency']['freeze'] = authorities['freeze']
    records['root']['emergency']['resume'] = authorities['resume']
    records['root']['upgrade']['authority'] = authorities['upgrade']
    return records


def genesis_values():
    """The approved E05 values with the staging-only notice."""
    values = read_json(E05_VALUES)
    rows = [row for row in values['values'] if row.get('path') in NOTICE_PATHS]
    require(len(rows) == len(NOTICE_PATHS), 'the notice values are missing')
    for row in rows:
        row['approved'], row['approval_record'] = STAGING_NOTICE_BLOCKS, DRILL_APPROVAL
    return values


def build_genesis(records, out, tools, show):
    """Resolve the inputs and run the production builder into out/."""
    out.mkdir()
    write_json(out / 'records.json', records)
    write_json(out / 'values.json', genesis_values())
    run([sys.executable, '-B', HERE / 'resolve_genesis_inputs.py', '--mode', 'production',
         '--values', out / 'values.json', '--proposals', GENESIS_PROPOSALS, '--records', out / 'records.json',
         '--inputs', out / 'inputs.json', '--resolution', out / 'resolution.json'], show)
    run([tools / 'dytallix-genesis-build', '--inputs', out / 'inputs.json', '--out', out / 'build'], show)
    return out / 'build'


def sign_root(chain, genesis, manifest_path, tools, out, show):
    """Five throwaway root genesis keys; three of five sign the bundle."""
    keys = out / 'root-keys'
    keys.mkdir(mode=0o700)
    publics = []
    for n in range(1, ROOT_KEYS + 1):
        run([tools / 'dytallix-root-sign', 'keygen', '-private-key-out', keys / f'key-{n}.private',
             '-public-key-out', keys / f'key-{n}.json'], show)
        publics.append(keys / f'key-{n}.json')
    policy = out / 'root-policy.json'
    run([tools / 'dytallix-root-sign', 'policy', '-chain-id', chain, '-out', policy, *publics], show)
    digests = json.loads(run([tools / 'dytallix-root-sign', 'digest',
                              '-native-genesis', genesis / 'native-genesis.json',
                              '-config', genesis / 'application-config.json',
                              '-engine-genesis', genesis / 'genesis.json',
                              '-release-manifest', manifest_path], show))
    bundle = digests['bundle_sha512']
    parts = []
    for n in ROOT_SIGNERS:
        part = keys / f'signature-{n}.json'
        run([tools / 'dytallix-root-sign', 'sign', '-policy', policy, '-private-key', keys / f'key-{n}.private',
             '-bundle-sha512', bundle, '-out', part], show)
        parts.append(part)
    signatures = out / 'root-signatures.json'
    run([tools / 'dytallix-root-sign', 'combine', '-policy', policy, '-out', signatures, *parts], show)
    run([tools / 'dytallix-root-sign', 'verify', '-policy', policy, '-signatures', signatures,
         '-bundle-sha512', bundle], show)
    return policy, signatures, bundle


def staging_values(snapshot_interval=None):
    """The approved E05 values, with a staging-only snapshot interval: a CI run
    cannot wait a day (17,280 blocks) for the sentry's first snapshot."""
    values = read_json(E05_VALUES)
    if snapshot_interval is not None:
        require(1 <= snapshot_interval <= 17280, 'the staging snapshot interval is 1 to 17,280 blocks')
        row = next(r for r in values['values'] if r.get('path') == 'service.snapshots.interval_blocks')
        row['approved'] = snapshot_interval
    return values


def build(plan_path, keys_dir, release_dir, tools, out, genesis_time=None, show=print, snapshot_interval=None):
    plan_path, keys_dir, release_dir, tools, out = map(Path, (plan_path, keys_dir, release_dir, tools, out))
    plan = read_json(plan_path)
    require(not out.exists(), f'{out} exists; use a new directory')
    out.mkdir(parents=True)
    genesis_time = genesis_time or datetime.now(timezone.utc).replace(microsecond=0).strftime('%Y-%m-%dT%H:%M:%SZ')
    chain = plan['chain_id']
    records = staging_records(plan, read_json(REHEARSAL / 'records.json'), genesis_time)
    show('== control kits (throwaway, five)')
    records = with_control_kits(records, control_kits(tools, out, show))

    show('== genesis, first build')
    first = build_genesis(records, out / 'genesis-first', tools, show)
    show('== release manifest')
    release = out / 'release'
    release.mkdir()
    shutil.copyfile(release_dir / 'BUILD_RECORD.json', release / 'BUILD_RECORD.json')
    manifest_path = release / 'RELEASE_MANIFEST.json'
    value = release_manifest.manifest(release_manifest.load_record(release / 'BUILD_RECORD.json'), chain,
                                      first / 'native-genesis.json')
    manifest_path.write_bytes(release_manifest.encode(value))
    release_sha512 = hashlib.sha512(manifest_path.read_bytes()).hexdigest()
    show(f'release manifest sha512 {release_sha512}')

    show('== genesis, bound to the release')
    records['root']['release_sha512'] = release_sha512
    genesis = build_genesis(records, out / 'genesis', tools, show)
    require((genesis / 'native-genesis.json').read_bytes() == (first / 'native-genesis.json').read_bytes(),
            'the native genesis changed when the release was bound')

    show('== root genesis signatures (throwaway keys, three of five)')
    policy, signatures, bundle = sign_root(chain, genesis, manifest_path, tools, out, show)
    chain_dir = out / 'chain'
    chain_dir.mkdir()
    for name in ('application-config.json', 'native-genesis.json', 'genesis.json'):
        shutil.copyfile(genesis / name, chain_dir / name)
    shutil.copyfile(policy, chain_dir / 'root-policy.json')
    shutil.copyfile(signatures, chain_dir / 'root-signatures.json')

    show('== host configuration')
    run([sys.executable, '-B', HERE / 'resolve_host_values.py', '--values', E05_VALUES, '--proposals', HOST_PROPOSALS,
         '--host-values', out / 'host-values.json', '--resolution', out / 'host-values-resolution.json'], show)
    run([tools / 'dytallix-host-config', '--plan', plan_path, '--values', out / 'host-values.json',
         '--genesis', chain_dir / 'genesis.json', '--out', out / 'hosts'], show)

    show('== host files and bundles')
    generated = host_files.generate(release, chain_dir, out / 'hosts', plan, keys_dir, staging_values(snapshot_interval),
                                    read_json(SETUP_VALUES))
    host_files.write(out / 'host-files', generated)
    bundles = out / 'bundles'
    bundles.mkdir()
    built = {}
    for label in generated:
        built[label] = host_bundle.build(out / 'host-files' / label, release_dir, keys_dir / f'{label}.sealed.json',
                                         bundles / f'{label}.bundle.tar')
        show(f'{label}: {built[label]["bundle"]} sha256 {built[label]["sha256"]}')
    summary = {'status': 'STAGING_CHAIN_BUILT', 'chain_id': chain, 'genesis_time': genesis_time,
               'release_sha512': release_sha512, 'root_bundle_sha512': bundle,
               'validators': [v['validator_id'] for v in records['validators']],
               'bundles': {label: {'path': b['bundle'], 'sha256': b['sha256'], 'role': b['role']}
                           for label, b in built.items()},
               'boundary': BOUNDARY}
    write_json(out / 'STAGING.json', summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for name in ('plan', 'keys', 'release', 'tools', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--genesis-time', help='UTC, whole seconds (default: now)')
    parser.add_argument('--snapshot-interval', type=int, help='staging only: the sentry snapshot interval in blocks')
    args = parser.parse_args()
    try:
        summary = build(args.plan, args.keys, args.release, args.tools, args.out, args.genesis_time,
                        snapshot_interval=args.snapshot_interval)
    except (Invalid, host_files.Invalid, host_bundle.Invalid, OSError, KeyError, ValueError) as error:
        print(f'staging_chain: {error}', file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
