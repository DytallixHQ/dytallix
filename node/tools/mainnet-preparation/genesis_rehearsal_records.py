#!/usr/bin/env python3
"""Synthetic records for the genesis rehearsal (E05-d). No real person, key, amount or approval.

Public keys are SHAKE-256 outputs of fixed labels, so the records are
reproducible, not key pairs: nothing can sign for them. The allocation follows
the approved shape (the whole 1,000,000,000 DGT in the five bucket shares, the
1,000,000 DRT bootstrap, four validators at the 100,000 DGT minimum self-bond)
with invented holders. Real records replace every row before any production
build.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path

DOMAIN = b'DYTALLIX/GENESIS-REHEARSAL/v1\0'
DGT, DRT = 10**6, 10**6
GENESIS_UNIX = 1798761600  # 2027-01-01T00:00:00Z, synthetic


def shake(label, size): return hashlib.shake_256(DOMAIN + label.encode()).digest(size)


def mldsa65(label): return base64.b64encode(shake(label, 1952)).decode()


def authority(prefix, count=5, threshold=3):
    keys = []
    for i in range(1, count + 1):
        raw = shake(f'{prefix}-{i}', 64)
        keys.append({'key_id': hashlib.sha256(raw).hexdigest(), 'public_key_hex': raw.hex()})
    keys.sort(key=lambda k: k['key_id'])
    return {'keys': keys, 'threshold': threshold} if threshold else keys


def account(label, dgt, drt, vesting=None):
    return {'label': label, 'origin_public_key_base64': mldsa65('account-' + label),
            'udgt': str(dgt * DGT), 'udrt': str(drt * DRT), 'vesting': vesting or {'kind': 'unlocked'}}


def records():
    operators = [f'operator-{i}' for i in range(1, 5)]
    team_vesting = {'kind': 'linear_after_cliff', 'total_amount': str(200_000_000 * DGT), 'start_time': GENESIS_UNIX,
                    'cliff_duration': 365 * 86400, 'vesting_duration': 4 * 365 * 86400, 'allow_staking': False}
    accounts = [
        account('ecosystem-growth', 300_000_000, 996_000),
        account('team-and-advisors', 200_000_000, 0, team_vesting),
        account('public-sale', 150_000_000, 0),
        account('private-sale', 150_000_000, 0),
        account('reserve', 199_600_000, 0),
    ] + [account(o, 100_000, 1_000) for o in operators]
    return {
        'schema': 'dytallix.genesis-records.v1',
        'status': 'SYNTHETIC',
        'boundary': 'Invented rehearsal records. Not beneficiaries, operators, custodians, keys or a chain identity. Replace every row with accepted records before any production build.',
        'chain_id': 'dytallix-rehearsal-1',
        'genesis_time': '2027-01-01T00:00:00Z',
        'network': 'mainnet',
        'accounts': accounts,
        'validators': [{'validator_id': f'validator-{i}', 'operator': o, 'consensus_public_key_base64': mldsa65(f'validator-{i}'),
                        'self_bond_udgt': str(100_000 * DGT)} for i, o in enumerate(operators, 1)],
        'delegations': [{'account': 'ecosystem-growth', 'validator_id': 'validator-1', 'amount_udgt': str(1_000_000 * DGT)}],
        'governance_action_classes': [{'class': c, 'approval_digest': hashlib.sha256(DOMAIN + f'class-{c}'.encode()).hexdigest()} for c in (1, 2)],
        'root': {
            'release_sha512': hashlib.sha512(DOMAIN + b'release').hexdigest(),
            'emergency': {'authority_epoch': 1, 'freeze': authority('emergency-freeze'), 'resume': authority('emergency-resume')},
            # The shape of the upgrade custodian intake's authority_fragment (E05-c).
            'upgrade': {'parameter_set': 'SLH-DSA-SHAKE-256s', 'authority_epoch': 1, 'authority': authority('upgrade')},
            'handover': {'authority_epoch': 1, 'keys': authority('handover', threshold=None)},
        },
    }


def document(reference, body):
    canonical = (json.dumps(body, sort_keys=True, ensure_ascii=True, separators=(',', ':')) + '\n').encode()
    return {'reference': reference, 'sha256': hashlib.sha256(canonical).hexdigest(), 'document': body}


def render(value): return json.dumps(value, indent=2, sort_keys=True) + '\n'


def review_packet(built):
    """The binding review's records and bindings for the rehearsal build in `built` (E05-d2).

    Returns (records text, bindings text). The records use the PRODUCTION_INPUTS
    row format with synthetic references; the bindings tie them to the built files.
    """
    source = records()
    native_raw = (built/'native-genesis.json').read_bytes()
    config_raw = (built/'application-config.json').read_bytes()
    engine_raw = (built/'genesis.json').read_bytes()
    native, config = json.loads(native_raw), json.loads(config_raw)
    addresses = {a['label']: n['address'] for a, n in zip(source['accounts'], native['accounts'])}
    operator_of = {v['validator_id']: f'operator-{i}' for i, v in enumerate(source['validators'], 1)}
    stakes = {}
    for v in source['validators']: stakes[v['operator']] = (operator_of[v['validator_id']], v['self_bond_udgt'])
    for d in source['delegations']: stakes[d['account']] = (operator_of[d['validator_id']], d['amount_udgt'])
    docs, allocation, bootstrap, account_bindings = [], [], [], []
    policy = 'approvals/P01_E05_VALUES_2_2026-09-30.json'
    for a in source['accounts']:
        label, locked = a['label'], a['vesting']['kind'] != 'unlocked'
        docs += [document('account:' + label, {'kind': 'account', 'address': addresses[label]}),
                 document('vesting:' + label, {'kind': 'vesting_terms', 'vesting': a['vesting']})]
        delegation = {'kind': 'none'}
        if label in stakes:
            operator, amount = stakes[label]
            delegation = {'kind': 'delegated', 'validator_operator_id': operator, 'amount_base_units': amount}
        allocation.append({'beneficiary_reference': 'synthetic:' + label, 'category': 'synthetic', 'recipient_account_reference': 'account:' + label,
                           'amount_base_units': a['udgt'], 'explicit_vesting_schedule': {'kind': 'explicit_schedule' if locked else 'none', 'approved_terms_reference': 'vesting:' + label},
                           'staking_permission': a['vesting']['allow_staking'] if locked else True, 'initial_delegation': delegation, 'custody_reference': None, 'approval_evidence': None})
        rows = []
        if a['udrt'] != '0':
            rows.append(len(bootstrap))
            bootstrap.append({'recipient_account_reference': 'account:' + label, 'amount_base_units': a['udrt'],
                              'approved_bootstrap_policy_reference': policy, 'custody_reference': None, 'approval_evidence': None})
        account_bindings.append({'address': addresses[label], 'allocation_row': len(allocation) - 1, 'bootstrap_rows': rows})
    operators = []
    for v in source['validators']:
        docs.append(document('validator-key:' + v['validator_id'], {'kind': 'validator_key', 'algorithm': 'ML-DSA-65', 'public_key_base64': v['consensus_public_key_base64']}))
        operators.append({'operator_id': operator_of[v['validator_id']], 'public_key_reference': 'validator-key:' + v['validator_id']})
    docs.append(document('identity:' + source['chain_id'], {'kind': 'identity_policy', 'chain_id': source['chain_id']}))
    packet = {
        'schema_version': 1,
        'approved_context': 'Synthetic rehearsal records (E05-d2). Not production inputs.',
        'acceptance_boundary': 'No record here is accepted; the rehearsal proves the review path only.',
        'records': {'D08-Q01': {'rows': allocation}, 'D08-Q03': {'rows': bootstrap}, 'D09-Q02': {'rows': operators},
                    'D13-Q02': {'rows': [{'approved_identity_policy_reference': 'identity:' + source['chain_id']}]}},
        'policy_inputs': {'dgt_initial_supply': {'amount_base_units': str(sum(int(a['udgt']) for a in source['accounts']))},
                          'drt_bootstrap': {'amount_base_units': str(sum(int(a['udrt']) for a in source['accounts'])), 'policy_approval_reference': policy}},
    }
    records_text = render(packet)
    engine = json.loads(engine_raw)
    bindings = {
        'schema_version': 1,
        'profile': 'production-runtime-review-only',
        'source_digests': {'records_sha256': hashlib.sha256(records_text.encode()).hexdigest(), 'native_genesis_sha256': hashlib.sha256(native_raw).hexdigest(),
                           'application_config_sha256': hashlib.sha256(config_raw).hexdigest(), 'engine_genesis_sha256': hashlib.sha256(engine_raw).hexdigest()},
        'runtime_inputs': {
            'chain_id': {'value': source['chain_id'], 'identity_record_row': 0},
            'genesis_time': engine['genesis_time'],
            'account_bindings': account_bindings,
            'validator_bindings': [{'reward_address': v['validator_id'], 'record_row': i} for i, v in enumerate(source['validators'])],
            'governance_parameters': config['governance'],
            'issuance_parameters': native['adaptive_issuance'],
            'reward_parameters': native['reward_v2'],
            # Bound by application_config_sha256; an exact copy would double the fixture.
            'consensus_configuration': None,
            # The rehearsal has no transport; the reviewed profile is loopback only until activation.
            'network_configuration': None,
            'root_authorization': None,
            'approval_bundle': None,
        },
        'public_documents': docs,
    }
    return records_text, render(bindings)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--out', type=Path, required=True, help='the rehearsal fixture directory')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    outputs = [(args.out/'records.json', render(records()))]
    if (args.out/'genesis.json').is_file():
        review_records, review_bindings = review_packet(args.out)
        outputs += [(args.out/'review-records.json', review_records), (args.out/'review-bindings.json', review_bindings)]
    if args.check:
        stale = [str(p) for p, text in outputs if not p.is_file() or p.read_text() != text]
        if stale: print('STALE: ' + ', '.join(stale)); return 1
    else:
        for path, text in outputs: path.write_text(text)
    return 0


if __name__ == '__main__': raise SystemExit(main())
