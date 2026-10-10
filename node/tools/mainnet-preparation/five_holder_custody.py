#!/usr/bin/env python3
"""Check the five key holders' custody packets together. Does not verify signatures or accept anything.

  five_holder_custody.py --emergency E --upgrade U --genesis G [--output F] [--policy-out F]

Five people hold one key kit each (P01, 7 October 2026); kit N holds key N
of the genesis, upgrade, freeze and resume roles
(launch/custody/KEY_CEREMONY.md). Each packet is checked by its own checker
under the five_holders model, then the three together: the same five
holders and kits in all three, slot N named only kit-holder-N in kit-N,
twenty distinct keys and one authority epoch for the emergency and upgrade
keys (a kit replacement moves a seat's keys together). No slot carries an
independence review: the public disclosure stands in for it (P01, 10
October 2026, D10-Q03).
"""
import argparse
import hashlib
import json
from pathlib import Path
import check_bindings as c
import emergency_custodian_intake as e
import genesis_signer_intake as g
import upgrade_custodian_intake as u

SCHEMA = 'dytallix.five-holder-custody-check.v1'
D10_Q03 = ('Approved: the five holders\' packets use the public disclosure in place of an independence review '
           '(P01_E05_HOLDER_DISCLOSURE_2026-10-10).')


def check(packets, roots):
    """packets and roots: {'emergency': ..., 'upgrade': ..., 'genesis': ...}."""
    emergency, upgrade, genesis = packets['emergency'], packets['upgrade'], packets['genesis']
    results = {'emergency': e.validate(emergency, roots['emergency']),
               'upgrade': u.validate(upgrade, roots['upgrade'], emergency),
               'genesis': g.validate(genesis, roots['genesis'], emergency, upgrade)}
    errors = [f'{name}: {error}' for name, result in results.items() for error in result['errors']]
    for name, packet in packets.items():
        if not (isinstance(packet, dict) and packet.get('custody_model') == u.FIVE):
            errors.append(f'{name}: custody_model must be {u.FIVE}')
    if isinstance(emergency, dict) and isinstance(upgrade, dict) and emergency.get('authority_epoch') != upgrade.get('authority_epoch'):
        errors.append('the emergency and upgrade keys must share one authority epoch: each kit\'s roles are replaced together')
    out = {'schema': SCHEMA, 'status': 'STRUCTURALLY_COMPLETE' if not errors else 'INCOMPLETE_OR_INVALID', 'errors': errors,
           'custody_model': u.FIVE, 'custody_review': 'public_disclosure' if not errors else None, 'd10_q03': D10_Q03,
           'packets': {name: result['status'] for name, result in results.items()},
           'signature_verification_performed': False, 'identity_or_independence_verified': False, 'production_accepted': False,
           'boundary': 'Checks the three packets structurally and together: five kit holders named only by their kits, the same '
                       'in every role, distinct keys and one epoch; no independence review (the public disclosure). Cryptographic verification, the holders\' '
                       'identities and formal acceptance remain required; holders\' names stay in the custody system.'}
    if not errors:
        out['emergency_authority_fragment'] = results['emergency']['authority_fragment']
        out['upgrade_authority_fragment'] = results['upgrade']['authority_fragment']
        out['signer_policy'] = results['genesis']['signer_policy']
        out['signer_policy_sha256'] = results['genesis']['signer_policy_sha256']
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for name in ('emergency', 'upgrade', 'genesis'):
        parser.add_argument('--' + name, type=Path, required=True, help=f'the working {name} intake packet')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--policy-out', type=Path, help='write the genesis signer policy when complete; never replaces a file')
    args = parser.parse_args()
    paths = {'emergency': args.emergency, 'upgrade': args.upgrade, 'genesis': args.genesis}
    try:
        read = {name: c.read(path) for name, path in paths.items()}
        output = check({name: data for name, (_, data) in read.items()}, {name: path.parent for name, path in paths.items()})
        output['packet_sha256'] = {name: hashlib.sha256(raw).hexdigest() for name, (raw, _) in read.items()}
    except (OSError, ValueError, TypeError) as exc:
        output = {'schema': SCHEMA, 'status': 'INCOMPLETE_OR_INVALID', 'errors': [f'input unreadable or invalid: {type(exc).__name__}'],
                  'production_accepted': False}
    if args.policy_out and 'signer_policy' in output:
        with args.policy_out.open('x', encoding='utf-8') as stream:
            stream.write(output['signer_policy'])
    rendered = json.dumps(output, indent=2) + '\n'
    if args.output: args.output.write_text(rendered)
    print(rendered, end='')
    return 0 if output['status'] == 'STRUCTURALLY_COMPLETE' else 2


if __name__ == '__main__': raise SystemExit(main())
