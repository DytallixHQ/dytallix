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


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    text = json.dumps(records(), indent=2, sort_keys=True) + '\n'
    if args.check:
        if not args.out.is_file() or args.out.read_text() != text: print('STALE'); return 1
    else:
        args.out.write_text(text)
    return 0


if __name__ == '__main__': raise SystemExit(main())
