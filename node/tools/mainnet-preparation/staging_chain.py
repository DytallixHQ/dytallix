#!/usr/bin/env python3
"""Build a throwaway production-profile staging chain and its host bundles
(E05; host setup v1, step H4: node/docs/architecture/host-setup-v1.md).

  staging_chain.py --plan PIN_PLAN.json --keys KEYS_DIR --release RELEASE_DIR \
                   --tools TOOLS_DIR --out OUT_DIR [--genesis-time TIME] \
                   [--timing production|rehearsal|ci] [--snapshot-interval N]

Inputs:
  --plan     the pin plan with public keys filled in (host_keys.py writes it);
             a staging chain ID, never the approved network identity
  --keys     host_keys.py's output: each host's key summary, sealed keys and
             each serving host's channel pin
  --release  a release build (BUILD_RECORD.json and bin/)
  --tools    production builds of dytallix-genesis-build and
             dytallix-host-config, and dytallix-root-sign
  --timing   the timing profile (simulation plan, T1): production, the exact
             values (R07; the default); rehearsal, R01 to R06's shortened set
             and the values derived from it, refused if a derived constraint
             breaks; ci, the CI staging network's (the drill's notice and
             --snapshot-interval). OUT_DIR/TIMING.json records every changed
             value beside its production value.

It runs the launch pipeline end to end with synthetic records, in the
release manifest's freeze order:
  1. records: the approved production allocation's shape (the five buckets,
     the treasury, the five root key holders' grants, each validator's
     operator with its self-bond, and the DRT bootstrap) on spendable
     rehearsal accounts, whose ML-DSA-65 keys the release's own CLI makes in
     rehearsal-wallets/ (simulation plan, T1), with this plan's validators
     (their keys from the key step), chain ID and genesis time;
  2. resolve and build the genesis (production builder);
  3. write the release manifest from the build record and native genesis;
  4. bind root.release_sha512 and build again (the native genesis must not
     change);
  5. make five throwaway root genesis keys and sign three of five;
     (before step 1, five throwaway key kits replace the rehearsal's
     freeze, resume and upgrade keys, which nothing can sign for, so that
     the kit replacement drill can sign controls; in the ci profile the
     upgrade and handover notice is a staging-only 20 blocks: P01, 8 October
     2026)
  6. resolve the host values and run dytallix-host-config;
  7. generate every host's files (host_files.py) and bundle them
     (host_bundle.py).

OUT_DIR receives each stage's files and bundles/LABEL.bundle.tar. Its
root-keys/, control-kits/ and rehearsal-wallets/ hold the throwaway private
keys (the wallets' keystore under rehearsal-wallets/home, its passphrase in
rehearsal-wallets/passphrase, and MAPPING.json naming the production row each
account stands for): this chain is for staging and CI only, its records and
keys are invented, and nothing here may be reused for any network.
"""
import argparse
import base64
import copy
from datetime import datetime, timezone
import hashlib
import json
import os
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
# Timing profiles (simulation plan, T1): production, the exact values (R07 and
# the default); rehearsal, R01 to R06's shortened set, so that each lifecycle
# completes inside a 24-hour run; ci, the CI staging network's (the drill's
# notice and a snapshot interval of its choosing).
PROFILES = ('production', 'rehearsal', 'ci')
SIMULATION_APPROVAL = 'launch/approvals/P01_T06_SIMULATION_PLAN_2026-10-11.json'
REHEARSAL_TIMINGS = {
    'upgrade.min_notice_blocks': 720,
    'release_handover.v2.min_notice_blocks': 720,
    'governance.deposit.deposit_period_blocks': 720,
    'governance.ballot.voting_period_blocks': 720,
    'governance.ballot.timelock_blocks': 360,
    'lifecycle.evidence_max_age_blocks': 1440,
    'lifecycle.processing_margin_blocks': 120,
    'service.snapshots.interval_blocks': 720,
}
# A restore trusts the copy's header for the evidence age less this
# (dytallix-pqc-engine restore.go, restoreTrustMargin; P01, 8 October 2026).
RESTORE_TRUST_MARGIN_SECONDS = 24 * 3600
# A stale backup is one older than the snapshot interval and this slack
# (monitor.backup_max_age_seconds: 26 hours with daily snapshots).
BACKUP_SLACK_SECONDS = 2 * 3600
RUN_SECONDS = 24 * 3600
BOUNDARY = ('Staging chain for host setup v1 (H4) and the launch simulations: the approved allocation\'s shape on '
            'throwaway accounts, validator and root keys. Not for any network.')
DGT, DRT = 10**6, 10**6
SELF_BOND_DGT = 100_000
HOLDERS = 5
HOLDER_GRANT_DGT = 20_000
# The approved allocation's rows (genesis framework, sections 1 and 2), each on
# a rehearsal account: label, the production row it stands for, DGT, DRT. The
# validators' operators (operator-N, each holding its self-bond) are carved out
# of Ecosystem growth, as the production self-bond is. The DRT bootstrap rows
# are open in production (D08-Q03): here Ecosystem growth holds what the other
# rows do not, to fund the simulations' wallets.
DRT_BOOTSTRAP = 1_000_000
REHEARSAL_ROWS = (
    ('ecosystem-growth', 'Ecosystem growth bucket, less the validators\' self-bonds', 300_000_000, None),
    ('team-and-advisors', 'Team and advisors bucket, less the root key holders\' grants',
     200_000_000 - HOLDERS * HOLDER_GRANT_DGT, 0),
    ('public-sale', 'Public sale bucket', 150_000_000, 0),
    ('private-sale', 'Private sale bucket', 150_000_000, 0),
    ('reserve', 'Reserve bucket', 200_000_000, 0),
    ('treasury', 'Treasury account', 0, 1_000),
) + tuple((f'kit-holder-{n}', f'Root key holder {n}\'s grant', HOLDER_GRANT_DGT, 2_000) for n in range(1, HOLDERS + 1))
OPERATOR_DRT = 1_000


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


def rehearsal_labels(validators):
    """Every rehearsal account, in the records' order."""
    return [row[0] for row in REHEARSAL_ROWS] + [f'operator-{n}' for n in range(1, validators + 1)]


def rehearsal_wallets(cli, out, labels, show):
    """One encrypted keystore holding an ML-DSA-65 key per rehearsal account,
    made by the release's own CLI as a user makes a wallet. Returns each
    label's public key; the private keys stay in the keystore."""
    wallets = out / 'rehearsal-wallets'
    home = wallets / 'home'
    home.mkdir(parents=True, mode=0o700)
    passphrase = wallets / 'passphrase'
    passphrase.write_text(hashlib.sha256(os.urandom(32)).hexdigest() + '\n')
    passphrase.chmod(0o600)
    env = dict(os.environ, HOME=str(home), DYTALLIX_KEYSTORE_PASSPHRASE_FILE=str(passphrase))
    for label in labels:
        show(f'$ dytallix wallet create --name {label}')
        result = subprocess.run([str(cli), 'wallet', 'create', '--name', label], capture_output=True, text=True, env=env)
        if result.returncode != 0:
            raise Invalid(f'dytallix wallet create failed: {(result.stderr or "").strip()}')
    return keystore_public_keys(home / '.dytallix' / 'keystore.json', labels)


def keystore_public_keys(path, labels):
    """Each label's public key from a keystore, which stores them in the clear."""
    entries = {e['name']: e for e in read_json(path)['entries']}
    require(set(labels) <= set(entries), f'the keystore lacks {sorted(set(labels) - set(entries))}')
    keys = {}
    for label in labels:
        raw = bytes(entries[label]['public_key'])
        require(len(raw) == 1952, f'{label} is not an ML-DSA-65 key')
        keys[label] = base64.b64encode(raw).decode()
    return keys


def spendable_records(plan, rehearsal, genesis_time, public_keys):
    """The staging records on spendable accounts: the approved allocation's
    rows with keys the harness holds, all unlocked, with no delegation."""
    records = staging_records(plan, rehearsal, genesis_time)
    validators = len(records['validators'])
    operators = [f'operator-{n}' for n in range(1, validators + 1)]
    for n, validator in enumerate(records['validators'], 1):
        validator['operator'], validator['self_bond_udgt'] = f'operator-{n}', str(SELF_BOND_DGT * DGT)
    others_drt = sum(drt for _, _, _, drt in REHEARSAL_ROWS if drt is not None) + OPERATOR_DRT * validators
    rows = [(label, dgt - SELF_BOND_DGT * validators if drt is None else dgt, DRT_BOOTSTRAP - others_drt if drt is None else drt)
            for label, _, dgt, drt in REHEARSAL_ROWS]
    rows += [(label, SELF_BOND_DGT, OPERATOR_DRT) for label in operators]
    records['accounts'] = [{'label': label, 'origin_public_key_base64': public_keys[label], 'udgt': str(dgt * DGT),
                            'udrt': str(drt * DRT), 'vesting': {'kind': 'unlocked'}} for label, dgt, drt in rows]
    records['delegations'] = []
    require(sum(int(a['udgt']) for a in records['accounts']) == 10**9 * DGT, 'the rehearsal accounts do not hold the whole DGT total')
    require(sum(int(a['udrt']) for a in records['accounts']) == DRT_BOOTSTRAP * DRT, 'the rehearsal accounts do not hold the DRT bootstrap')
    return records


def mapping(records, native_genesis, chain):
    """Each rehearsal account's address and the production row it stands for."""
    stands_for = {label: row for label, row, _, _ in REHEARSAL_ROWS}
    native = native_genesis['accounts']
    require(len(native) == len(records['accounts']), 'the native genesis has another number of accounts')
    accounts = []
    for account, built in zip(records['accounts'], native):
        label = account['label']
        row = stands_for[label] if label in stands_for else f"Validator {label.split('-')[1]}'s operator, holding its self-bond"
        accounts.append({'label': label, 'wallet': label, 'address': built['address'], 'stands_for': row,
                         'udgt': account['udgt'], 'udrt': account['udrt']})
    return {'schema': 'dytallix.rehearsal-accounts.v1', 'chain_id': chain, 'boundary': BOUNDARY,
            'keystore': 'home/.dytallix/keystore.json', 'passphrase': 'passphrase', 'accounts': accounts}


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


def seconds(text):
    """A single-unit duration (the host values' form) in whole seconds."""
    units = {'s': 1, 'm': 60, 'h': 3600}
    require(text[-1:] in units and text[:-1].isdigit(), f'not a duration in whole seconds: {text}')
    return int(text[:-1]) * units[text[-1]]


def duration(total):
    return f'{total // 3600}h' if total % 3600 == 0 else f'{total}s'


def derived(chosen, block_seconds):
    """The values tied to the chosen block values, by the rules that give the
    approved production values from theirs: each evidence and margin age in
    seconds is its blocks at the block interval, the peer state-sync trust
    period is half the evidence age (below it, as the engine requires), and
    a backup is stale after the snapshot interval and the slack."""
    evidence = chosen['lifecycle.evidence_max_age_blocks'] * block_seconds
    return {
        'lifecycle.evidence_max_age_seconds': evidence,
        'lifecycle.processing_margin_seconds': chosen['lifecycle.processing_margin_blocks'] * block_seconds,
        'config.toml [statesync] trust_period': duration(evidence // 2),
        'monitor.backup_max_age_seconds': chosen['service.snapshots.interval_blocks'] * block_seconds + BACKUP_SLACK_SECONDS,
    }


DERIVATION_RULES = {
    'lifecycle.evidence_max_age_seconds': 'evidence age blocks x block interval; equals the engine evidence max_age_duration',
    'lifecycle.processing_margin_seconds': 'processing margin blocks x block interval',
    'config.toml [statesync] trust_period': 'half the evidence age; below it (dytallix-pqc-engine, dytallix-host-config)',
    'monitor.backup_max_age_seconds': 'snapshot interval x block interval + 2 hours',
}


def violations(values, block_seconds, profile):
    """The derived constraints a set of timings breaks (none for a usable set)."""
    found = []
    evidence = values['lifecycle.evidence_max_age_seconds']
    trust = seconds(values['config.toml [statesync] trust_period'])
    if not 0 < trust < evidence:
        found.append(f'the state-sync trust period ({trust} s) is not below the evidence age ({evidence} s)')
    restore = evidence - RESTORE_TRUST_MARGIN_SECONDS
    if restore < values['monitor.backup_max_age_seconds']:
        found.append(f'a restore trusts a copy for the evidence age less {RESTORE_TRUST_MARGIN_SECONDS} s (the engine\'s '
                     f'restore margin): {evidence} s leaves {restore} s, less than a backup\'s '
                     f'{values["monitor.backup_max_age_seconds"]} s freshness, so the restore drills would be refused')
    if profile == 'rehearsal':
        run_blocks = RUN_SECONDS // block_seconds
        lifecycles = {
            'a governance proposal (deposit, voting, timelock)': values['governance.deposit.deposit_period_blocks']
            + values['governance.ballot.voting_period_blocks'] + values['governance.ballot.timelock_blocks'],
            'an unbond (evidence age and margin)': values['lifecycle.evidence_max_age_blocks']
            + values['lifecycle.processing_margin_blocks'],
            'an upgrade notice': values['upgrade.min_notice_blocks'],
            'a handover notice': values['release_handover.v2.min_notice_blocks'],
            'a snapshot': values['service.snapshots.interval_blocks'],
        }
        found += [f'{name} takes {blocks} blocks, not inside a {run_blocks}-block run'
                  for name, blocks in lifecycles.items() if blocks >= run_blocks]
        unbond = values['lifecycle.evidence_max_age_seconds'] + values['lifecycle.processing_margin_seconds']
        if unbond >= RUN_SECONDS:
            found.append(f'an unbond takes {unbond} s, not inside a 24-hour run')
    return found


def timing(profile='production', snapshot_interval=None):
    """The E05 values with a timing profile applied, and its record: every
    changed value with its production value, source and rule. Refuses a
    profile whose derived constraints do not hold."""
    require(profile in PROFILES, f'the timing profile is one of {", ".join(PROFILES)}')
    require(snapshot_interval is None or profile == 'ci', 'only the ci profile takes a snapshot interval')
    values = read_json(E05_VALUES)
    rows = {row.get('path'): row for row in values['values']}
    block_seconds = values['block_interval_assumption_seconds']
    production = {path: row.get('approved') for path, row in rows.items()}
    changes = []

    def change(path, value, source, approval, rule):
        require(path in rows, f'the E05 value {path} is missing')
        # Kept in the production value's form: large integers are strings.
        value = str(value) if isinstance(production[path], str) else value
        rows[path]['approved'], rows[path]['approval_record'] = value, approval
        changes.append({'path': path, 'production': production[path], 'value': value, 'source': source, 'rule': rule})

    if profile == 'ci':
        for path in NOTICE_PATHS:
            change(path, STAGING_NOTICE_BLOCKS, 'ci', DRILL_APPROVAL, 'the kit replacement drill\'s notice')
        if snapshot_interval is not None:
            require(1 <= snapshot_interval <= 17280, 'the staging snapshot interval is 1 to 17,280 blocks')
            row = rows['service.snapshots.interval_blocks']
            row['approved'] = snapshot_interval
            changes.append({'path': row['path'], 'production': production[row['path']], 'value': snapshot_interval,
                            'source': 'ci', 'rule': 'a CI run cannot wait a day for the first snapshot'})
    chosen = {path: int(value) if isinstance(value, str) and value.isdigit() else value
              for path, value in production.items()}
    if profile == 'rehearsal':
        chosen.update(REHEARSAL_TIMINGS)
        for path, value in REHEARSAL_TIMINGS.items():
            change(path, value, 'plan', SIMULATION_APPROVAL, 'shortened timings, R01 to R06')
        for path, value in derived(chosen, block_seconds).items():
            change(path, value, 'derived', SIMULATION_APPROVAL, DERIVATION_RULES[path])
            chosen[path] = value
    found = [] if profile == 'ci' else violations(chosen, block_seconds, profile)
    require(not found, f'the {profile} timing profile breaks a derived constraint: ' + '; '.join(found))
    record = {'schema': 'dytallix.rehearsal-timing.v1', 'profile': profile,
              'runs': {'production': 'R07, and any run at the exact production values', 'rehearsal': 'R01 to R06',
                       'ci': 'the CI staging network only'}[profile],
              'block_interval_seconds': block_seconds, 'values': changes, 'boundary': BOUNDARY}
    return values, record


def build_genesis(records, out, tools, values, show):
    """Resolve the inputs and run the production builder into out/."""
    out.mkdir()
    write_json(out / 'records.json', records)
    write_json(out / 'values.json', values)
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


def build(plan_path, keys_dir, release_dir, tools, out, genesis_time=None, show=print, snapshot_interval=None,
          profile='production'):
    plan_path, keys_dir, release_dir, tools, out = map(Path, (plan_path, keys_dir, release_dir, tools, out))
    plan = read_json(plan_path)
    require(not out.exists(), f'{out} exists; use a new directory')
    values, timing_record = timing(profile, snapshot_interval)
    out.mkdir(parents=True)
    write_json(out / 'values.json', values)
    write_json(out / 'TIMING.json', timing_record)
    show(f'== timing profile: {profile} ({timing_record["runs"]}), {len(timing_record["values"])} values changed')
    genesis_time = genesis_time or datetime.now(timezone.utc).replace(microsecond=0).strftime('%Y-%m-%dT%H:%M:%SZ')
    chain = plan['chain_id']
    validators = len([h for h in plan['hosts'] if h['role'] == 'validator'])
    show('== rehearsal wallets (throwaway, spendable)')
    public_keys = rehearsal_wallets(release_dir / 'bin' / 'dytallix', out, rehearsal_labels(validators), show)
    records = spendable_records(plan, read_json(REHEARSAL / 'records.json'), genesis_time, public_keys)
    show('== control kits (throwaway, five)')
    records = with_control_kits(records, control_kits(tools, out, show))

    show('== genesis, first build')
    first = build_genesis(records, out / 'genesis-first', tools, values, show)
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
    genesis = build_genesis(records, out / 'genesis', tools, values, show)
    require((genesis / 'native-genesis.json').read_bytes() == (first / 'native-genesis.json').read_bytes(),
            'the native genesis changed when the release was bound')
    write_json(out / 'rehearsal-wallets' / 'MAPPING.json', mapping(records, read_json(genesis / 'native-genesis.json'), chain))

    show('== root genesis signatures (throwaway keys, three of five)')
    policy, signatures, bundle = sign_root(chain, genesis, manifest_path, tools, out, show)
    chain_dir = out / 'chain'
    chain_dir.mkdir()
    for name in ('application-config.json', 'native-genesis.json', 'genesis.json'):
        shutil.copyfile(genesis / name, chain_dir / name)
    shutil.copyfile(policy, chain_dir / 'root-policy.json')
    shutil.copyfile(signatures, chain_dir / 'root-signatures.json')

    show('== host configuration')
    run([sys.executable, '-B', HERE / 'resolve_host_values.py', '--values', out / 'values.json', '--proposals', HOST_PROPOSALS,
         '--host-values', out / 'host-values.json', '--resolution', out / 'host-values-resolution.json'], show)
    run([tools / 'dytallix-host-config', '--plan', plan_path, '--values', out / 'host-values.json',
         '--genesis', chain_dir / 'genesis.json', '--out', out / 'hosts'], show)

    show('== host files and bundles')
    generated = host_files.generate(release, chain_dir, out / 'hosts', plan, keys_dir, values,
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
               'timing': {'profile': profile, 'record': 'TIMING.json',
                          'sha256': hashlib.sha256((out / 'TIMING.json').read_bytes()).hexdigest()},
               'boundary': BOUNDARY}
    write_json(out / 'STAGING.json', summary)
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for name in ('plan', 'keys', 'release', 'tools', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--genesis-time', help='UTC, whole seconds (default: now)')
    parser.add_argument('--timing', choices=PROFILES, default='production',
                        help='production (R07, the default), rehearsal (R01 to R06) or ci')
    parser.add_argument('--snapshot-interval', type=int, help='ci profile only: the sentry snapshot interval in blocks')
    args = parser.parse_args()
    try:
        summary = build(args.plan, args.keys, args.release, args.tools, args.out, args.genesis_time,
                        snapshot_interval=args.snapshot_interval, profile=args.timing)
    except (Invalid, host_files.Invalid, host_bundle.Invalid, OSError, KeyError, ValueError) as error:
        print(f'staging_chain: {error}', file=sys.stderr)
        return 2
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
