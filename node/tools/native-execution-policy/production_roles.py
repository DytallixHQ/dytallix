#!/usr/bin/env python3
"""Render four-role service policy candidates. This does not load or accept policy."""
import argparse
import copy
import hashlib
import json
from pathlib import Path

import render as base

VERSION = 'native-four-role-stack-candidate-v4'
MAX_UNITS = 16
CONTROLLER = 'unconfined'
ORDINARY = tuple('hup int quit ill trap abrt bus fpe kill usr1 segv usr2 pipe alrm '
                 'term stkflt chld stp ttin ttou urg xcpu xfsz vtalrm prof winch '
                 'io pwr sys emt exists'.split()) + tuple('rtmin+' + str(i) for i in range(33))
CONTROL = ('stop', 'cont')
CLEANUP = ('term', 'kill', 'cont')
ROLE_ORDER = ('supervisor', 'application-owner', 'workload', 'helper')


def record(value):
    return (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()


def require_profiles(matrix, loaded_enforce_labels):
    """Pure model. The service must read fresh kernel state before startup."""
    expected = {label for row in matrix['units']
                for label in row['required_enforce_profiles']}
    base.require(type(loaded_enforce_labels) is list and
                 len(set(loaded_enforce_labels)) == len(loaded_enforce_labels),
                 'Unique loaded profile labels required')
    base.require(expected <= set(loaded_enforce_labels),
                 'Required enforce profile missing')
    return True


def role_of(catalog_role):
    return {'service_supervisor': 'supervisor',
            'consensus_stdio': 'application-owner',
            'genesis_bootstrap_verifier': 'helper',
            'control_verifier': 'helper'}.get(catalog_role, 'workload')


def effective_labels(components):
    """Canonical labels observed on Linux 6.8 with AppArmor ABI 4.0."""
    s, a, w, h = (components[role] for role in ROLE_ORDER)
    return {'supervisor': s, 'application-owner': a + '//&' + s,
            'workload': s + '//&' + w,
            'helper': a + '//&' + h + '//&' + s}


def generate(catalog_bytes, mapping, request, unit_identities, *, candidate_only=False):
    base.require(candidate_only is True, 'Explicit candidate-only invocation required')
    base.require(type(unit_identities) is list, 'Explicit unit identity list required')
    for row in unit_identities:
        base.keys(row, ['unit', 'uid', 'gid'], 'unit identity')
        base.require(all(type(row[key]) is int and 1 <= row[key] <= 4294967294
                         for key in ('uid', 'gid')), 'Non-root UID/GID required')
    identities = {row['unit']: row for row in unit_identities}
    units = [row['unit'] for row in unit_identities]
    base.require(1 <= len(units) <= MAX_UNITS, 'Unit count out of bounds')
    base.unique(units, 'Unit identifiers')
    for unit in units:
        base.identifier(unit)
        base.require(len(unit) <= 32, 'Unit identifier too long')
    units.sort()
    normal = base.render(catalog_bytes, mapping, request)
    catalog, members, paths, *_ = base.validate(catalog_bytes, mapping, request)
    base.require({row['uid'] for row in unit_identities} == set(request['service_uids']),
                 'Unit UIDs must match declared service UIDs')
    base.require(request['network'].get('launch_channel') == base.LAUNCH_CHANNEL,
                 'Exact Rust launch channel required')
    roles = {row['role']: row['member_id'] for row in catalog['roles']}
    base.require('service_supervisor' in roles and 'consensus_stdio' in roles and
                 'genesis_bootstrap_verifier' in roles and 'control_verifier' in roles,
                 'Required roles absent')
    member_roles = {}
    for catalog_role, member_id in roles.items():
        role = role_of(catalog_role)
        base.require(member_id not in member_roles or member_roles[member_id] == role,
                     'One executable cannot serve different security roles')
        member_roles[member_id] = role
    role_paths = {role: set() for role in ROLE_ORDER}
    for member_id, role in member_roles.items():
        base.require(members[member_id]['kind'] == 'executable',
                     'Role member must be executable')
        role_paths[role].update(paths[member_id])
    base.require(all(role_paths.values()), 'Each security role needs an executable')
    all_executables = {path for values in role_paths.values() for path in values}
    base.require(sum(map(len, role_paths.values())) == len(all_executables),
                 'Executable path crosses security roles')
    binding = {'version': VERSION, 'launch_peer_model': 'four-role-additive-stack-v1',
               'normal_manifest_sha256': hashlib.sha256(normal['FILE_HASHES.json']).hexdigest(),
               'units': [identities[unit] for unit in units],
               'trusted_controller_label': CONTROLLER}
    identity = hashlib.sha256(base.canonical(binding)).hexdigest()
    prefix = 'dyt-role-' + identity[:20]
    matrix = {'schema': 3, 'scope': VERSION, 'candidate_only': True,
              'identity_sha256': identity, 'binding': binding,
              'trusted_controller_label': CONTROLLER,
              'host_administrator_is_trusted': True,
              'kernel_enforcement_verified': False,
              'exec': [], 'signal': [], 'ptrace': [], 'launch_channel': [], 'units': []}
    normal_properties = json.loads(normal['unit-properties.json'])
    remove = {'  ' + path + ' rmix,' for path in all_executables}
    retained = []
    for line in normal['apparmor.profile'].decode().splitlines():
        if line in remove or line.startswith(('abi ', '#', 'profile ')) or line in ('}', ''):
            continue
        if line.startswith(('  ptrace ', '  signal ', '  unix ')):
            continue
        retained.append(line)
    profiles = ['abi <abi/4.0>,',
                '# FOUR-ROLE SOURCE CANDIDATE ONLY. Kernel and service qualification required.',
                '# Additive Px stacks. No inheritance or unconfined fallback.']
    properties, admissions = [], {}
    for unit in units:
        components = {role: prefix + '-' + unit + '-' + role for role in ROLE_ORDER}
        labels = effective_labels(components)
        matrix['units'].append({'unit': unit,
                                **{role + '_label': labels[role] for role in ROLE_ORDER},
                                'required_enforce_profiles': list(components.values())})
        props = copy.deepcopy(normal_properties['properties'])
        props['AppArmorProfile'] = labels['supervisor']
        props['User'] = str(identities[unit]['uid'])
        props['Group'] = str(identities[unit]['gid'])
        properties.append({'unit': unit, 'properties': props,
                           'required_enforce_profiles': list(components.values()),
                           'merge_rule': normal_properties['merge_rule']})
        admissions[unit] = {'schema': 2, 'unit': unit,
            'policy_identity_sha256': identity,
            'catalog_sha512': hashlib.sha512(catalog_bytes).hexdigest(),
            'uid': identities[unit]['uid'], 'gid': identities[unit]['gid'],
            'supervisor_label': labels['supervisor'],
            'application_owner_label': labels['application-owner'],
            'workload_label': labels['workload'], 'helper_label': labels['helper'],
            'no_new_privileges': 1, 'seccomp': 2,
            'mount_namespace': 'inherit-supervisor',
            'roles': [{'role': name, 'member_id': roles[name],
                       'label': labels[role_of(name)]} for name in sorted(roles)]}
        for role in ROLE_ORDER:
            label = components[role]
            lines = ['profile ' + label + ' {'] + retained
            lines += ['  owner /proc/[0-9]*/fdinfo/8 r,',
                      '  owner /proc/[0-9]*/ns/mnt r,']
            for executable in sorted(all_executables):
                target = next(name for name in ROLE_ORDER if executable in role_paths[name])
                if target == role:
                    if role == 'helper':
                        lines.append('  ' + executable + ' rm,')
                        continue
                    lines.append('  ' + executable + ' rmix,')
                    mode, target_label = 'ix', labels[role]
                elif (role, target) in (('supervisor', 'application-owner'),
                                       ('supervisor', 'workload'),
                                       ('supervisor', 'helper'),
                                       ('application-owner', 'helper')):
                    source_component = components[role]
                    target_component = components[target]
                    lines.append('  ' + executable + ' rmPx -> ' + source_component +
                                 '//&' + target_component + ',')
                    mode = 'Px-stack'
                    target_label = (components['helper'] + '//&' + components['supervisor']
                                    if role == 'supervisor' and target == 'helper'
                                    else labels[target])
                else:
                    # Read-only metadata cannot execute or map code. The base
                    # profile supplies no broad executable grant.
                    lines.append('  ' + executable + ' r,')
                    continue
                matrix['exec'].append({'source_profile': label, 'path': executable,
                                       'mode': mode, 'target_label': target_label,
                                       'required_profiles': list(components.values()),
                                       'helper_entry_guard_required': role == 'supervisor' and
                                       target == 'helper'})

            def signal(access, names, peer):
                lines.append('  signal (' + access + ') set=(' + ', '.join(names) +
                             ') peer=' + peer + ',')
                matrix['signal'].append({'source_profile': label, 'access': access,
                                         'signals': list(names), 'peer': peer})

            def ptrace(access, peer):
                lines.append('  ptrace (' + access + ') peer=' + peer + ',')
                matrix['ptrace'].append({'source_profile': label, 'access': access,
                                         'peer': peer})

            signal('send', ORDINARY, labels[role])
            signal('receive', ORDINARY, labels[role])
            signal('receive', CLEANUP, CONTROLLER)
            signal('send', ('chld',), CONTROLLER)
            if role == 'supervisor':
                for target in ('application-owner', 'workload'):
                    signal('send', CONTROL + ('term', 'kill', 'exists'), labels[target])
                    ptrace('read, readby', labels[target])
                # The supervisor component remains on all children. Its
                # permissions must permit the allowed stacked peer pairs.
                signal('send', CONTROL + ('term', 'kill', 'exists'), labels['helper'])
                signal('receive', CONTROL + ('term', 'kill', 'exists'), labels['supervisor'])
                signal('receive', CONTROL + ('term', 'kill', 'exists'),
                       labels['application-owner'])
                signal('receive', ('chld',), labels['application-owner'])
                signal('receive', ('chld',), labels['workload'])
            elif role == 'application-owner':
                signal('receive', CONTROL + ('term', 'kill', 'exists'), labels['supervisor'])
                signal('receive', CONTROL + ('term', 'kill', 'exists'),
                       labels['application-owner'])
                signal('send', CONTROL + ('term', 'kill', 'exists'), labels['helper'])
                signal('send', ('chld',), labels['supervisor'])
                signal('receive', ('chld',), labels['helper'])
                ptrace('readby', labels['supervisor'])
                ptrace('read, readby', labels['helper'])
            elif role == 'workload':
                lines.append('  deny signal (send) set=(stop, cont),')
                signal('receive', CONTROL + ('term', 'kill', 'exists'), labels['supervisor'])
                signal('send', ('chld',), labels['supervisor'])
                ptrace('readby', labels['supervisor'])
            else:
                lines.append('  deny signal (send) set=(stop, cont),')
                signal('receive', CONTROL + ('term', 'kill', 'exists'), labels['application-owner'])
                signal('send', ('chld',), labels['application-owner'])
                ptrace('readby', labels['application-owner'])
            # AppArmor checks inherited AF_UNIX sockets against individual
            # stack components as well as the effective peer label. The bare
            # component labels below are exact names within this unit.
            peers = {'supervisor': (labels['supervisor'], labels['application-owner'],
                                    labels['workload'], labels['helper'],
                                    components['helper'], components['application-owner']),
                     'application-owner': (labels['application-owner'],
                                            labels['supervisor'], labels['helper'],
                                            components['application-owner'],
                                            components['helper']),
                     'workload': (labels['workload'], labels['supervisor']),
                     'helper': (labels['helper'], labels['application-owner'],
                                labels['supervisor'],
                                components['application-owner'])}[role]
            lines += ['  unix (create) type=seqpacket,',
                      '  unix (getattr, getopt) type=seqpacket addr=none,']
            for peer in peers:
                lines.append('  unix (send, receive) type=seqpacket addr=none '
                             'peer=(addr=none,label=' + peer + '),')
                matrix['launch_channel'].append({'source_profile': label, 'peer': peer,
                                                 'type': 'seqpacket',
                                                 'address': 'none', 'peer_address': 'none'})
            profiles += lines + ['  # A direct supervisor-to-helper exec yields an incomplete stack.',
                                 '  # The helper must reject every label without the owner component.',
                                 '  # No change_profile, capability or fallback execution grants.', '}']
    requirements = {'scope': VERSION, 'verified': False, 'native_execution': False,
        'required': [
            'Load all four exact profiles per unit in enforce mode before service startup.',
            'Confirm additive Px stack transitions with NoNewPrivileges enabled on the selected kernel.',
            'Prove the helper rejects the supervisor-to-helper incomplete label before processing input.',
            'Prove supervisor STOP/CONT only to its application and workload; application owner STOP/CONT only to its helper.',
            'Prove workload and helper cannot send STOP/CONT; prove cross-unit denial.',
            'Check exact catalog, helper file, profile, UID/GID, mount namespace and security state at admission.',
            'Retain original file, mapping, syscall, namespace and cleanup limits.',
            'This render does not qualify kernel behavior or production launch.'
        ],
        'base_requirements': json.loads(normal['live-verification-requirements.json'])['mandatory']}
    files = {'apparmor.profile': ('\n'.join(profiles) + '\n').encode(),
             'role-matrix.json': record(matrix),
             'unit-properties.json': record({'schema': 2, 'scope': VERSION,
                                             'candidate_only': True, 'units': properties}),
             'live-verification-requirements.json': record(requirements),
             'validation.json': record({'schema': 2, 'status': 'PRODUCTION_CANDIDATE_RENDER_ONLY',
                                        'profile_count': len(units) * 4,
                                        'identity_sha256': identity,
                                        'kernel_policy_qualified': False,
                                        'g35_accepted': False,
                                        'production_qualified': False}),
             'base-FILE_HASHES.json': normal['FILE_HASHES.json']}
    for name, raw in normal.items():
        files['base-' + name] = raw
    files['normalized-request.json'] = normal['normalized-request.json']
    for unit, admission in admissions.items():
        files['admission-' + unit + '.json'] = record(admission)
    files['FILE_HASHES.json'] = record({name: {'bytes': len(raw),
                                            'sha256': hashlib.sha256(raw).hexdigest()}
                                      for name, raw in sorted(files.items())})
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--candidate-only', action='store_true', required=True)
    for name in ('catalog', 'mapping', 'request', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--unit-identities', type=Path, required=True)
    args = parser.parse_args()
    try:
        files = generate(args.catalog.read_bytes(), base.decode(args.mapping.read_bytes()),
                         base.decode(args.request.read_bytes()), base.decode(args.unit_identities.read_bytes()),
                         candidate_only=args.candidate_only)
        args.output.mkdir(parents=False, exist_ok=False)
        for name, raw in files.items():
            with (args.output / name).open('xb') as stream:
                stream.write(raw)
    except (base.Invalid, OSError, TypeError, KeyError) as error:
        parser.exit(2, 'Role candidate policy rendering failed: ' + str(error) + '\n')
    print(json.dumps({'status': 'PRODUCTION_CANDIDATE_RENDER_ONLY', 'files': len(files),
                      'output': str(args.output), 'native_execution': False}))


if __name__ == '__main__':
    main()
