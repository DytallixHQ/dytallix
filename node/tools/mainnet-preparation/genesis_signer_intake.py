#!/usr/bin/env python3
"""Check the public genesis signer intake. Does not verify signatures or accept anything."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import check_bindings as c
import upgrade_custodian_intake as u

SCHEMA = 'dytallix.genesis-signer-intake.v1'
# P01, 30 September 2026: the root genesis is signed three of five by its own
# group of genesis signers, separate from the emergency and upgrade
# custodians (production activation v1, decision 2), with SLH-DSA-SHAKE-256s.
THRESHOLD, SIZE = 3, 5
PARAMETER_SET = u.PARAMETER_SET
KEY_BYTES = u.KEY_BYTES
PURPOSE = 'genesis'
# The signer policy record (root_genesis/threshold.rs, genesis_threshold.go).
POLICY_SCHEMA = 1
CHAIN_ID = re.compile(r'^[A-Za-z0-9._-]{1,128}$')
TOP = {'schema', 'production_accepted', 'custody_model', 'threshold', 'authority_size', 'chain_id', 'profile', 'signers', 'evidence'}
MODELS = u.MODELS
PERSON = u.PERSON
KEY = {'key_id', 'public_key_hex', 'signer_control_group', 'backup_control_group', 'signer_record', 'backup_record', 'proof_of_possession', 'drill_record'}
HEX_KEY = re.compile(r'^[a-f0-9]{128}$')


def text(value): return u.text(value)


def public_key(value):
    """Lowercase hex of exactly KEY_BYTES non-zero bytes, as dytallix-root-sign keygen writes it, or None."""
    if not isinstance(value, str) or HEX_KEY.fullmatch(value) is None: return None
    raw = bytes.fromhex(value)
    return raw if any(raw) else None


def upgrade_holdings(packet):
    """Controllers, control groups and public keys of a complete upgrade custodian intake, or an error."""
    if not isinstance(packet, dict) or packet.get('schema') != u.SCHEMA:
        return None, 'upgrade intake: schema mismatch'
    people = packet.get('custodians')
    if not isinstance(people, list) or len(people) != SIZE:
        return None, 'upgrade intake incomplete: five custodians required to check separation'
    controllers, groups, keys = set(), set(), set()
    for person in people:
        if not isinstance(person, dict) or not text(person.get('controller_id')) or not text(person.get('control_group')):
            return None, 'upgrade intake incomplete: controllers and control groups required to check separation'
        controllers.add(person['controller_id']); groups.add(person['control_group'])
        key = person.get('key')
        raw = u.public_key(key.get('public_key_base64')) if isinstance(key, dict) else None
        if raw is None:
            return None, 'upgrade intake incomplete: public keys required to check separation'
        keys.add(raw)
        groups.update(g for g in (key.get('signer_control_group'), key.get('backup_control_group')) if text(g))
    return (controllers, groups, keys), None


def signer_policy(chain_id, authority):
    """The public signer policy record in the exact bytes `dytallix-root-sign policy` writes and the node reads."""
    record = {'schema': POLICY_SCHEMA, 'chain_id': chain_id,
              'authority': {'keys': sorted(authority, key=lambda k: k['key_id']), 'threshold': THRESHOLD}}
    return json.dumps(record, separators=(',', ':'), ensure_ascii=False)


def validate(data, root, emergency=None, upgrade=None):
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
    solo, five = data['custody_model'] == 'solo_kits', data['custody_model'] == u.FIVE
    require(data['custody_model'] in MODELS, 'custody_model must be independent, solo_kits or five_holders')
    chain = data['chain_id']
    require(isinstance(chain, str) and CHAIN_ID.fullmatch(chain) is not None, 'chain_id: the chain identity record (D13-Q01) is required')
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
        if not require(path.is_file() and path.stat().st_size <= u.EVIDENCE_LIMIT, identifier + ': missing or oversized public evidence'): continue
        raw = path.read_bytes()
        if not require(isinstance(ref['sha256'], str) and u.HEX.fullmatch(ref['sha256']) is not None and hashlib.sha256(raw).hexdigest() == ref['sha256'], identifier + ': digest mismatch'): continue
        try: item = c.decode(raw)
        except (ValueError, UnicodeError):
            require(False, identifier + ': evidence must be UTF-8 JSON'); continue
        if not fields(item, u.ITEM, identifier): continue
        if not require(item['kind'] in u.KINDS and text(item['public_statement']), identifier + ': invalid public statement'): continue
        parsed[identifier] = item

    used = set()

    def binding(ref, kind, controller=None, key=None, group=None):
        if not require(text(ref) and ref in parsed, f'{controller or "profile"}.{kind}: missing bound evidence'): return
        used.add(ref)
        item = parsed[ref]
        keyed = kind in u.KEY_EVIDENCE
        # A genesis signer policy has no authority epoch: its keys sign one
        # genesis per chain, so keyed evidence binds the purpose, not an epoch.
        require(item['kind'] == kind and item['controller_id'] == controller
                and item['purpose'] == (PURPOSE if keyed else None) and item['key_id'] == key
                and item['parameter_set'] == PARAMETER_SET and item['epoch'] is None,
                ref + ': subject, purpose, key, epoch or profile mismatch')
        if kind == 'independence_review':
            require(text(item['reviewer_control_group']) and item['reviewer_control_group'] != group, ref + ': self-review cannot establish independence')

    binding(profile['approval'], 'profile_approval')
    people = data['signers']
    if not require(isinstance(people, list) and len(people) == SIZE, 'exactly five genesis signers required'):
        return result(errors)
    others = []
    for name, packet, holdings in (('emergency', emergency, u.emergency_holdings), ('upgrade', upgrade, upgrade_holdings)):
        held, problem = holdings(packet) if packet is not None else (None, name + ' intake required to check separation')
        require(held is not None, problem)
        if held is not None: others.append((name, held))
        if five and held is not None:
            require(packet.get('custody_model') == u.FIVE, f'five_holders: the {name} intake uses another custody model')
    groups, controllers, keys, slots, authority = set(), set(), set(), set(), []
    for person in people:
        if not fields(person, PERSON, 'signer'): continue
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
        elif five:
            # The public disclosure stands in for the review (P01, 10 October 2026, D10-Q03).
            require(person['independence_review'] is None, f'slot {slot}: five_holders has no independence review (public disclosure)')
        else:
            binding(person['independence_review'], 'independence_review', controller, group=group)
        key = person['key']
        if not fields(key, KEY, f'slot {slot}.key'): continue
        raw = public_key(key['public_key_hex'])
        require(raw is not None, f'slot {slot}: public key must be {KEY_BYTES} bytes of lowercase hex')
        fingerprint = key['key_id']
        require(raw is not None and fingerprint == hashlib.sha256(raw).hexdigest(), f'slot {slot}: key identifier must be SHA-256 of public bytes')
        require(text(fingerprint) and fingerprint not in keys, f'slot {slot}: reused or missing key')
        if text(fingerprint): keys.add(fingerprint)
        require(text(group) and key['signer_control_group'] == group and key['backup_control_group'] == group, f'slot {slot}: signer or backup control crosses signer groups')
        for kind in u.KEY_EVIDENCE:
            binding(key[kind], kind, controller, fingerprint)
        # Genesis signers are their own group (P01, 30 September 2026); the
        # node also refuses a genesis key with another root role.
        for name, (o_controllers, o_groups, o_keys) in others:
            if not solo and not five:
                require(controller not in o_controllers, f'slot {slot}: controller is also an {name} custodian')
                require(group not in o_groups and key['signer_control_group'] not in o_groups and key['backup_control_group'] not in o_groups, f'slot {slot}: control group is shared with an {name} custodian')
            require(raw not in o_keys, f'slot {slot}: key also holds an {name} role')
        if raw is not None: authority.append({'key_id': fingerprint, 'public_key_hex': raw.hex()})
    if solo:
        require(len(controllers) == 1, 'solo_kits: one controller holds every slot')
        for name, (o_controllers, o_groups, _) in others:
            require(o_controllers == controllers, f'solo_kits: the {name} intake names another controller')
            require(o_groups == groups, f'solo_kits: the {name} keys use other kits')
    if five:
        for name, (o_controllers, o_groups, _) in others:
            require(o_controllers == controllers and o_groups == groups, f'five_holders: the {name} intake names other holders or kits')
    for ref, item in parsed.items():
        if item['kind'] == 'independence_review':
            require(text(item['reviewer_control_group']) and item['reviewer_control_group'] not in groups, ref + ': reviewer shares a signer control group')
    require(set(parsed) == used, 'unreferenced evidence is not permitted')
    out = result(errors)
    out['custody_model'] = data['custody_model'] if data['custody_model'] in MODELS else None
    if five and not errors: out['custody_review'] = 'public_disclosure'
    if not errors:
        policy = signer_policy(chain, authority)
        out['signer_policy'] = policy
        out['signer_policy_sha256'] = hashlib.sha256(policy.encode()).hexdigest()
    return out


def result(errors):
    return {'status': 'STRUCTURALLY_COMPLETE' if not errors else 'INCOMPLETE_OR_INVALID', 'errors': errors,
            'signature_verification_performed': False, 'identity_or_independence_verified': False, 'production_accepted': False,
            'boundary': 'Checks syntax, separation from the emergency and upgrade custodians (or, for solo kits, one controller with the same five kits in all three; for five holders, the same five kit holders in all three) and local public evidence bindings only. Cryptographic verification and formal acceptance remain required.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('packet', type=Path)
    parser.add_argument('--emergency', type=Path, help='the working emergency custodian intake, to check separation')
    parser.add_argument('--upgrade', type=Path, help='the working upgrade custodian intake, to check separation')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--policy-out', type=Path, help='write the signer policy record when complete; never replaces a file')
    args = parser.parse_args()
    try:
        _, data = c.read(args.packet)
        emergency = c.read(args.emergency)[1] if args.emergency else None
        upgrade = c.read(args.upgrade)[1] if args.upgrade else None
        output = validate(data, args.packet.parent, emergency, upgrade)
    except (OSError, ValueError, TypeError) as exc:
        output = result([f'input unreadable or invalid: {type(exc).__name__}'])
    if args.policy_out and 'signer_policy' in output:
        with args.policy_out.open('x', encoding='utf-8') as stream:
            stream.write(output['signer_policy'])
    rendered = json.dumps(output, indent=2) + '\n'
    if args.output: args.output.write_text(rendered)
    print(rendered, end='')
    return 0 if output['status'] == 'STRUCTURALLY_COMPLETE' else 2


if __name__ == '__main__': raise SystemExit(main())
