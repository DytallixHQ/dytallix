#!/usr/bin/env python3
"""Check the public upgrade custodian intake. Does not verify signatures or accept anything."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import check_bindings as c

SCHEMA = 'dytallix.upgrade-custodian-intake.v1'
EMERGENCY_SCHEMA = 'dytallix.emergency-custodian-intake.v2'
# P01, 30 September 2026: three of five custodians (D11-Q03) and SLH-DSA-SHAKE-256s,
# the set the node's root verifier implements (root_genesis.rs, emergency_verifier.rs).
THRESHOLD, SIZE = 3, 5
PARAMETER_SET = 'SLH-DSA-SHAKE-256s'
KEY_BYTES = 64  # upgrade/v1/upgrade.rs KEY_BYTES
PURPOSE = 'upgrade'
EVIDENCE_LIMIT = 1024*1024
TOP = {'schema', 'production_accepted', 'custody_model', 'threshold', 'authority_size', 'authority_epoch', 'profile', 'custodians', 'evidence'}
# `independent`: five custodians in separate control groups (P01, 30 September
# 2026). `solo_kits`: the solo launch profile (P01, 3 October 2026), in which the
# founder holds all five slots in five key kits, each kit its own control group.
# `five_holders`: five people, one kit each (P01, 7 October 2026): slot N is
# kit N in the emergency, upgrade and genesis packets alike, named only by its
# kit, so the roles share their holders by design. As under the solo
# profile, the public disclosure stands in for an independence review (P01,
# 10 October 2026, D10-Q03).
FIVE = 'five_holders'
MODELS = ('independent', 'solo_kits', FIVE)
PERSON = {'slot', 'controller_id', 'name', 'organization', 'control_group', 'appointment', 'independence_review', 'key'}
KEY = {'key_id', 'public_key_base64', 'signer_control_group', 'backup_control_group', 'signer_record', 'backup_record', 'proof_of_possession', 'drill_record'}
ITEM = {'kind', 'controller_id', 'purpose', 'key_id', 'parameter_set', 'epoch', 'reviewer_control_group', 'public_statement'}
KEY_EVIDENCE = ('signer_record', 'backup_record', 'proof_of_possession', 'drill_record')
KINDS = {'profile_approval', 'appointment', 'independence_review', *KEY_EVIDENCE}
HEX = re.compile(r'^[a-f0-9]{64}$')


def text(value): return isinstance(value, str) and bool(value.strip())


def kit_names(slot):
    """Slot N's controller and control group under five_holders: the kit, never the person."""
    return f'kit-holder-{slot}', f'kit-{slot}'



def public_key(value):
    """Canonical base64 of exactly KEY_BYTES non-zero bytes, or None."""
    if not isinstance(value, str): return None
    try: raw = base64.b64decode(value, validate=True)
    except (ValueError, TypeError): return None
    return raw if len(raw) == KEY_BYTES and base64.b64encode(raw).decode() == value and any(raw) else None


def emergency_holdings(packet):
    """Controllers, control groups and public keys of a complete emergency intake, or an error."""
    if not isinstance(packet, dict) or packet.get('schema') != EMERGENCY_SCHEMA:
        return None, 'emergency intake: schema mismatch'
    people = packet.get('custodians')
    if not isinstance(people, list) or len(people) != SIZE:
        return None, 'emergency intake incomplete: five custodians required to check separation'
    controllers, groups, keys = set(), set(), set()
    for person in people:
        if not isinstance(person, dict) or not text(person.get('controller_id')) or not text(person.get('control_group')):
            return None, 'emergency intake incomplete: controllers and control groups required to check separation'
        controllers.add(person['controller_id']); groups.add(person['control_group'])
        pair = person.get('keys')
        if not isinstance(pair, dict) or set(pair) != {'freeze', 'resume'}:
            return None, 'emergency intake incomplete: freeze and resume keys required to check separation'
        for key in pair.values():
            raw = public_key(key.get('public_key_base64')) if isinstance(key, dict) else None
            if raw is None:
                return None, 'emergency intake incomplete: public keys required to check separation'
            keys.add(raw)
            groups.update(g for g in (key.get('signer_control_group'), key.get('backup_control_group')) if text(g))
    return (controllers, groups, keys), None


def validate(data, root, emergency=None):
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
    for key, value in (('threshold', THRESHOLD), ('authority_size', SIZE)):
        require(type(data[key]) is int and data[key] == value, key + ': approved value mismatch')
    solo, five = data['custody_model'] == 'solo_kits', data['custody_model'] == FIVE
    require(data['custody_model'] in MODELS, 'custody_model must be independent, solo_kits or five_holders')
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

    def binding(ref, kind, controller=None, key=None, group=None):
        if not require(text(ref) and ref in parsed, f'{controller or "profile"}.{kind}: missing bound evidence'): return
        used.add(ref)
        item = parsed[ref]
        keyed = kind in KEY_EVIDENCE
        require(item['kind'] == kind and item['controller_id'] == controller
                and item['purpose'] == (PURPOSE if keyed else None) and item['key_id'] == key
                and item['parameter_set'] == PARAMETER_SET and item['epoch'] == (epoch if keyed else None),
                ref + ': subject, purpose, key, epoch or profile mismatch')
        if kind == 'independence_review':
            require(text(item['reviewer_control_group']) and item['reviewer_control_group'] != group, ref + ': self-review cannot establish independence')

    binding(profile['approval'], 'profile_approval')
    people = data['custodians']
    if not require(isinstance(people, list) and len(people) == SIZE, 'exactly five custodians required'):
        return result(errors)
    held, problem = emergency_holdings(emergency) if emergency is not None else (None, 'emergency intake required to check separation')
    require(held is not None, problem)
    if five and held is not None:
        require(emergency.get('custody_model') == FIVE, 'five_holders: the emergency intake uses another custody model')
    groups, controllers, keys, slots, authority = set(), set(), set(), set(), []
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
            require((controller, group) == kit_names(slot), f'slot {slot}: five_holders names the holder only by the kit: '
                    f'controller {kit_names(slot)[0]}, control group {kit_names(slot)[1]}')
        require(text(group) and group not in groups, f'slot {slot}: duplicate or missing control group')
        if text(controller): controllers.add(controller)
        if text(group): groups.add(group)
        binding(person['appointment'], 'appointment', controller)
        if solo:
            # The public disclosure (TRUST_MODEL.md) replaces the independence review.
            require(person['independence_review'] is None, f'slot {slot}: solo_kits has no independence review')
        elif five:
            # The public disclosure stands in for the review (P01, 10 October 2026, D10-Q03).
            require(person['independence_review'] is None, f'slot {slot}: five_holders has no independence review (public disclosure)')
        else:
            binding(person['independence_review'], 'independence_review', controller, group=group)
        key = person['key']
        if not fields(key, KEY, f'slot {slot}.key'): continue
        raw = public_key(key['public_key_base64'])
        require(raw is not None, f'slot {slot}: invalid public-key encoding or size')
        fingerprint = key['key_id']
        require(raw is not None and fingerprint == hashlib.sha256(raw).hexdigest(), f'slot {slot}: key identifier must be SHA-256 of public bytes')
        require(text(fingerprint) and fingerprint not in keys, f'slot {slot}: reused or missing key')
        if text(fingerprint): keys.add(fingerprint)
        require(text(group) and key['signer_control_group'] == group and key['backup_control_group'] == group, f'slot {slot}: signer or backup control crosses custodian groups')
        for kind in KEY_EVIDENCE:
            binding(key[kind], kind, controller, fingerprint)
        if held is not None:
            e_controllers, e_groups, e_keys = held
            if not solo and not five:
                require(controller not in e_controllers, f'slot {slot}: controller is also an emergency custodian')
                require(group not in e_groups and key['signer_control_group'] not in e_groups and key['backup_control_group'] not in e_groups, f'slot {slot}: control group is shared with an emergency custodian')
            require(raw not in e_keys, f'slot {slot}: key also holds a freeze or resume role')
        if raw is not None: authority.append({'key_id': fingerprint, 'public_key_hex': raw.hex()})
    if solo:
        require(len(controllers) == 1, 'solo_kits: one controller holds every slot')
        if held is not None:
            require(held[0] == controllers, 'solo_kits: the emergency intake names another controller')
            require(held[1] == groups, 'solo_kits: the emergency keys use other kits')
    if five and held is not None:
        require(held[0] == controllers and held[1] == groups, 'five_holders: the emergency intake names other holders or kits')
    for ref, item in parsed.items():
        if item['kind'] == 'independence_review':
            require(text(item['reviewer_control_group']) and item['reviewer_control_group'] not in groups, ref + ': reviewer shares a custodian control group')
    require(set(parsed) == used, 'unreferenced evidence is not permitted')
    out = result(errors)
    out['custody_model'] = data['custody_model'] if data['custody_model'] in MODELS else None
    if five and not errors: out['custody_review'] = 'public_disclosure'
    if not errors:
        # The shape of the node's upgrade policy authority (upgrade/v1/upgrade.rs): keys strictly sorted by key_id.
        out['authority_fragment'] = {'parameter_set': PARAMETER_SET, 'authority_epoch': epoch,
                                     'authority': {'keys': sorted(authority, key=lambda k: k['key_id']), 'threshold': THRESHOLD}}
    return out


def result(errors):
    return {'status': 'STRUCTURALLY_COMPLETE' if not errors else 'INCOMPLETE_OR_INVALID', 'errors': errors,
            'signature_verification_performed': False, 'identity_or_independence_verified': False, 'production_accepted': False,
            'boundary': 'Checks syntax, emergency separation (or, for solo kits, one controller with the same five kits; for five holders, the same five kit holders) and local public evidence bindings only. Cryptographic verification and formal acceptance remain required.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('packet', type=Path)
    parser.add_argument('--emergency', type=Path, help='the working emergency custodian intake, to check separation')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    try:
        _, data = c.read(args.packet)
        emergency = c.read(args.emergency)[1] if args.emergency else None
        output = validate(data, args.packet.parent, emergency)
    except (OSError, ValueError, TypeError) as exc:
        output = result([f'input unreadable or invalid: {type(exc).__name__}'])
    rendered = json.dumps(output, indent=2) + '\n'
    if args.output: args.output.write_text(rendered)
    print(rendered, end='')
    return 0 if output['status'] == 'STRUCTURALLY_COMPLETE' else 2


if __name__ == '__main__': raise SystemExit(main())
