#!/usr/bin/env python3
"""Check the public emergency custodian intake. Does not verify signatures or accept anything.

The emergency authority freezes user transactions and resumes them
(docs/mainnet/emergency-transaction-freeze.md). Each of five custodians
holds one freeze key and one separate resume key; three signatures act for
each purpose. This intake comes first: the upgrade and genesis intakes check
their separation against it.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import check_bindings as c
import upgrade_custodian_intake as u

# v2 is this repository's packet: one authority epoch for both purposes, as the
# node's emergency policy has, and the custody model. The v1 template of
# September 2026 was never filled.
SCHEMA = 'dytallix.emergency-custodian-intake.v2'
# P01, 30 September 2026: three of five per purpose (D11-Q03) and
# SLH-DSA-SHAKE-256s, the set the node's root verifier implements.
THRESHOLD, SIZE = u.THRESHOLD, u.SIZE
PARAMETER_SET = u.PARAMETER_SET
PURPOSES = ('freeze', 'resume')
TOP = {'schema', 'production_accepted', 'custody_model', 'freeze_threshold', 'resume_threshold', 'authority_size',
       'authority_epoch', 'profile', 'custodians', 'evidence'}
PERSON = {'slot', 'controller_id', 'name', 'organization', 'control_group', 'appointment', 'independence_review', 'keys'}
KEY, ITEM, KEY_EVIDENCE, KINDS = u.KEY, u.ITEM, u.KEY_EVIDENCE, u.KINDS
EVIDENCE_LIMIT = u.EVIDENCE_LIMIT
HEX = re.compile(r'^[a-f0-9]{64}$')
text, public_key = u.text, u.public_key


def validate(data, root):
    errors = []

    def require(ok, message):
        if not ok: errors.append(message)
        return ok

    def fields(obj, expected, location):
        return require(isinstance(obj, dict) and set(obj) == expected, location + ': exact fields required')

    if not fields(data, TOP, 'intake'):
        return result(errors)
    require(data['schema'] == SCHEMA, 'schema mismatch')
    require(data['production_accepted'] is False, 'production acceptance must remain false')
    for key, value in (('freeze_threshold', THRESHOLD), ('resume_threshold', THRESHOLD), ('authority_size', SIZE)):
        require(type(data[key]) is int and data[key] == value, key + ': approved value mismatch')
    solo, five = data['custody_model'] == 'solo_kits', data['custody_model'] == u.FIVE
    require(data['custody_model'] in u.MODELS, 'custody_model must be independent, solo_kits or five_holders')
    epoch = data['authority_epoch']
    require(type(epoch) is int and epoch >= 1, 'explicit positive authority epoch required')
    profile = data['profile']
    if not fields(profile, {'parameter_set', 'approval'}, 'profile'):
        return result(errors)
    require(profile['parameter_set'] == PARAMETER_SET, 'parameter set must be the approved ' + PARAMETER_SET)

    evidence = data['evidence']
    if not require(isinstance(evidence, dict), 'evidence must be an object'):
        return result(errors)
    parsed = {}
    for identifier, ref in evidence.items():
        if not fields(ref, {'path', 'sha256'}, 'evidence.' + identifier): continue
        relative = ref['path']
        if not require(text(relative) and not Path(relative).is_absolute() and '..' not in Path(relative).parts, identifier + ': unsafe path'): continue
        path = root / relative
        if not require(not any(p.is_symlink() for p in [path, *path.parents] if p != root.parent) and path.resolve().is_relative_to(root.resolve()), identifier + ': path escape or symlink'): continue
        if not require(path.is_file() and path.stat().st_size <= EVIDENCE_LIMIT, identifier + ': missing or oversized public evidence'): continue
        raw = path.read_bytes()
        if not require(isinstance(ref['sha256'], str) and HEX.fullmatch(ref['sha256']) is not None and hashlib.sha256(raw).hexdigest() == ref['sha256'], identifier + ': digest mismatch'): continue
        try: item = c.decode(raw)
        except (ValueError, UnicodeError):
            require(False, identifier + ': evidence must be UTF-8 JSON'); continue
        if not fields(item, ITEM, identifier): continue
        if not require(item['kind'] in KINDS and text(item['public_statement']), identifier + ': invalid public statement'): continue
        parsed[identifier] = item

    used = set()

    def binding(ref, kind, controller=None, purpose=None, key=None, group=None):
        if not require(text(ref) and ref in parsed, f'{controller or "profile"}.{purpose or kind}: missing bound {kind} evidence'): return
        used.add(ref)
        item = parsed[ref]
        keyed = kind in KEY_EVIDENCE
        require(item['kind'] == kind and item['controller_id'] == controller
                and item['purpose'] == (purpose if keyed else None) and item['key_id'] == key
                and item['parameter_set'] == PARAMETER_SET and item['epoch'] == (epoch if keyed else None),
                ref + ': subject, purpose, key, epoch or profile mismatch')
        if kind == 'independence_review':
            require(text(item['reviewer_control_group']) and item['reviewer_control_group'] != group, ref + ': self-review cannot establish independence')

    binding(profile['approval'], 'profile_approval')
    people = data['custodians']
    if not require(isinstance(people, list) and len(people) == SIZE, 'exactly five custodians required'):
        return result(errors)
    groups, controllers, keys, slots, reviewed = set(), set(), set(), set(), []
    authority = {purpose: [] for purpose in PURPOSES}
    for person in people:
        if not fields(person, PERSON, 'custodian'): continue
        slot = person['slot']
        require(type(slot) is int and 1 <= slot <= SIZE and slot not in slots, 'distinct slots 1 through 5 required')
        if type(slot) is int: slots.add(slot)
        controller, group = person['controller_id'], person['control_group']
        for field in ('controller_id', 'name', 'organization', 'control_group'):
            require(text(person[field]), f'slot {slot}: {field} missing')
        # Solo kits: one controller in every slot, each slot its own kit.
        require(text(controller) and (solo or controller not in controllers), f'slot {slot}: duplicate or missing controller')
        if five:
            require((controller, group) == u.kit_names(slot), f'slot {slot}: five_holders names the holder only by the kit: '
                    f'controller {u.kit_names(slot)[0]}, control group {u.kit_names(slot)[1]}')
        require(text(group) and group not in groups, f'slot {slot}: duplicate or missing control group')
        if text(controller): controllers.add(controller)
        if text(group): groups.add(group)
        binding(person['appointment'], 'appointment', controller)
        if solo:
            # The public disclosure (TRUST_MODEL.md) replaces the independence review.
            require(person['independence_review'] is None, f'slot {slot}: solo_kits has no independence review')
        elif five and person['independence_review'] is None:
            reviewed.append(False)
        else:
            binding(person['independence_review'], 'independence_review', controller, group=group)
            if five: reviewed.append(True)
        pair = person['keys']
        if not fields(pair, set(PURPOSES), f'slot {slot}.keys'): continue
        for purpose in PURPOSES:
            key = pair[purpose]
            if not fields(key, KEY, f'slot {slot}.{purpose}'): continue
            raw = public_key(key['public_key_base64'])
            require(raw is not None, f'slot {slot}.{purpose}: invalid public-key encoding or size')
            fingerprint = key['key_id']
            require(raw is not None and fingerprint == hashlib.sha256(raw).hexdigest(), f'slot {slot}.{purpose}: key identifier must be SHA-256 of public bytes')
            # Freeze and resume keys are distinct everywhere (the node refuses a shared key).
            require(text(fingerprint) and fingerprint not in keys, f'slot {slot}.{purpose}: reused or missing key')
            if text(fingerprint): keys.add(fingerprint)
            require(text(group) and key['signer_control_group'] == group and key['backup_control_group'] == group, f'slot {slot}.{purpose}: signer or backup control crosses custodian groups')
            for kind in KEY_EVIDENCE:
                binding(key[kind], kind, controller, purpose, fingerprint)
            if raw is not None: authority[purpose].append({'key_id': fingerprint, 'public_key_hex': raw.hex()})
    if solo:
        require(len(controllers) == 1, 'solo_kits: one controller holds every slot')
    review = u.five_holder_review(require, reviewed) if five else None
    for ref, item in parsed.items():
        if item['kind'] == 'independence_review':
            require(text(item['reviewer_control_group']) and item['reviewer_control_group'] not in groups, ref + ': reviewer shares a custodian control group')
    require(set(parsed) == used, 'unreferenced evidence is not permitted')
    out = result(errors)
    out['custody_model'] = data['custody_model'] if data['custody_model'] in u.MODELS else None
    if five and not errors: out['custody_review'] = review
    if not errors:
        # records.root.emergency for the genesis resolver: the node's emergency
        # policy authorities, keys strictly sorted by key_id.
        out['authority_fragment'] = {'authority_epoch': epoch, **{
            purpose: {'keys': sorted(authority[purpose], key=lambda k: k['key_id']), 'threshold': THRESHOLD}
            for purpose in PURPOSES}}
    return out


def result(errors):
    return {'status': 'STRUCTURALLY_COMPLETE' if not errors else 'INCOMPLETE_OR_INVALID', 'errors': errors,
            'signature_verification_performed': False, 'identity_or_independence_verified': False, 'production_accepted': False,
            'boundary': 'Checks syntax, distinct freeze and resume keys, custodian control groups (or, for solo kits, one controller with five kits; for five holders, kit-holder-N in kit-N) and local public evidence bindings only. Cryptographic verification and formal acceptance remain required.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('packet', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    try:
        _, data = c.read(args.packet)
        output = validate(data, args.packet.parent)
    except (OSError, ValueError, TypeError) as exc:
        output = result([f'input unreadable or invalid: {type(exc).__name__}'])
    rendered = json.dumps(output, indent=2) + '\n'
    if args.output: args.output.write_text(rendered)
    print(rendered, end='')
    return 0 if output['status'] == 'STRUCTURALLY_COMPLETE' else 2


if __name__ == '__main__': raise SystemExit(main())
