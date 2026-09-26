#!/usr/bin/env python3
"""Render a qualification-only role policy. Never load profiles or run a service."""
import argparse
import copy
import hashlib
import json
from pathlib import Path

import render as base

VERSION = 'owned-child-role-separation-qualification-v1'
MAX_UNITS = 16
CONTROLLER = 'unconfined'
# Preserve ordinary runtime self-signals, including Go's URG and POSIX RT signals.
# STOP and CONT have separate, directed authority. No signal permission is implicit.
ORDINARY = tuple('hup int quit ill trap abrt bus fpe kill usr1 segv usr2 pipe alrm '
                 'term stkflt chld stp ttin ttou urg xcpu xfsz vtalrm prof winch '
                 'io pwr sys emt exists'.split()) + tuple('rtmin+' + str(i) for i in range(33))
CONTROL = ('stop', 'cont')
CLEANUP = ('term', 'kill', 'cont')


def record(value):
    return (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()


def require_profiles(matrix, loaded_enforce_labels):
    """Pure admission model. Caller must obtain fresh kernel evidence separately."""
    expected = {row[k] for row in matrix['units'] for k in ('supervisor', 'workload')}
    base.require(type(loaded_enforce_labels) is list and
                 len(set(loaded_enforce_labels)) == len(loaded_enforce_labels),
                 'Unique loaded profile labels required')
    base.require(expected <= set(loaded_enforce_labels),
                 'Complete required profile set in enforce mode is missing')
    return True


def generate(catalog_bytes, mapping, request, units, *, qualification_only=False):
    base.require(qualification_only is True, 'Explicit qualification-only invocation required')
    base.require(type(units) is list and 1 <= len(units) <= MAX_UNITS,
                 'Qualification unit count out of bounds')
    for unit in units:
        base.identifier(unit)
        base.require(len(unit) <= 32, 'Qualification unit identifier too long')
    base.unique(units, 'Qualification units')
    units = sorted(units)
    normal = base.render(catalog_bytes, mapping, request)
    catalog, members, paths, *_ = base.validate(catalog_bytes, mapping, request)
    base.require(request['network'].get('launch_channel') == base.LAUNCH_CHANNEL,
                 'Exact Rust launch channel required')
    roles = {r['role']: r['member_id'] for r in catalog['roles']}
    supervisor_id = roles['service_supervisor']
    base.require(all(mid != supervisor_id for role, mid in roles.items()
                     if role != 'service_supervisor'),
                 'Supervisor executable cannot serve a workload role')
    supervisor_paths = set(paths[supervisor_id])
    executables = {p for mid in members if members[mid]['kind'] == 'executable'
                   for p in paths[mid]}
    workload_paths = executables - supervisor_paths
    base.require(bool(workload_paths), 'Workload executable set is empty')
    binding = {'version': VERSION, 'normal_manifest_sha256':
               hashlib.sha256(normal['FILE_HASHES.json']).hexdigest(),
               'units': units, 'trusted_controller_label': CONTROLLER}
    identity = hashlib.sha256(base.canonical(binding)).hexdigest()
    prefix = 'dyt-qual-observe-' + identity[:20]
    matrix = {'schema': 1, 'scope': VERSION, 'qualification_only': True,
              'identity_sha256': identity, 'binding': binding,
              'trusted_controller_label': CONTROLLER,
              'host_administrator_is_trusted': True,
              'kernel_enforcement_verified': False,
              'exec': [], 'signal': [], 'ptrace': [], 'launch_channel': [], 'units': []}
    normal_properties = json.loads(normal['unit-properties.json'])
    normal_lines = normal['apparmor.profile'].decode().splitlines()
    # Retain every data, API-file, device, network and explicit trace-denial rule.
    # Replace only executable rules and the old one-label peer grants.
    remove = {'  ' + p + ' rmix,' for p in executables}
    retained = []
    for line in normal_lines:
        if line in remove or line.startswith(('abi ', '#', 'profile ')) or line in ('}', ''):
            continue
        if line.startswith(('  ptrace ', '  signal ', '  unix ')):
            continue
        retained.append(line)
    profiles = ['abi <abi/4.0>,', '# QUALIFICATION ONLY. No host action or approval.',
                '# Required Px transitions. No inheritance or unconfined fallback.']
    properties = []
    for unit in units:
        sup, work = prefix + '-' + unit + '-supervisor', prefix + '-' + unit + '-workload'
        matrix['units'].append({'unit': unit, 'supervisor': sup, 'workload': work})
        props = copy.deepcopy(normal_properties['properties'])
        props['AppArmorProfile'] = sup
        properties.append({'unit': unit, 'properties': props,
                           'required_enforce_profiles': [sup, work],
                           'merge_rule': normal_properties['merge_rule']})
        for label, role in ((sup, 'supervisor'), (work, 'workload')):
            lines = ['profile ' + label + ' {'] + retained
            for p in sorted(executables):
                if p in supervisor_paths:
                    if role == 'supervisor':
                        lines.append('  ' + p + ' rmix,')
                        matrix['exec'].append({'source': label, 'path': p,
                                               'mode': 'ix', 'target': label})
                    else:
                        lines += ['  ' + p + ' r,', '  deny ' + p + ' xm,']
                elif role == 'supervisor':
                    lines.append('  ' + p + ' rPx -> ' + work + ',')
                    matrix['exec'].append({'source': label, 'path': p,
                                           'mode': 'Px', 'target': work})
                else:
                    lines.append('  ' + p + ' rmix,')
                    matrix['exec'].append({'source': label, 'path': p,
                                           'mode': 'ix', 'target': label})

            def signal(access, signals, peer):
                lines.append('  signal (' + access + ') set=(' + ', '.join(signals) +
                             ') peer=' + peer + ',')
                matrix['signal'].append({'source_label': label, 'access': access,
                                         'signals': list(signals), 'peer': peer})

            def ptrace(access, peer):
                lines.append('  ptrace (' + access + ') peer=' + peer + ',')
                matrix['ptrace'].append({'source_label': label, 'access': access, 'peer': peer})

            # Self-signals remain exact-label grants. Workloads cannot STOP/CONT
            # themselves, helpers, siblings or any other unit.
            signal('send', ORDINARY, label)
            signal('receive', ORDINARY, label)
            signal('receive', CLEANUP, CONTROLLER)
            signal('send', ('chld',), CONTROLLER)
            if role == 'supervisor':
                signal('send', CONTROL + ('term', 'kill', 'exists'), work)
                signal('receive', ('chld',), work)
                ptrace('read, readby', sup)
                ptrace('read', work)
                peers = (sup, work)
            else:
                lines.append('  deny signal (send) set=(stop, cont),')
                signal('receive', CONTROL + ('term', 'kill', 'exists'), sup)
                signal('send', ('chld',), sup)
                ptrace('read, readby', work)
                ptrace('readby', sup)
                peers = (sup, work)
            lines.append('  unix (create) type=seqpacket,')
            for peer in peers:
                lines.append('  unix (send, receive) type=seqpacket addr=none '
                             'peer=(addr=none,label=' + peer + '),')
                matrix['launch_channel'].append({'source_label': label, 'peer': peer,
                                                 'type': 'seqpacket',
                                                 'address': 'none', 'peer_address': 'none'})
            lines += ['  # No change_profile, capability or fallback execution grants.', '}']
            profiles += lines
    requirements = {
        'scope': VERSION, 'verified': False, 'native_execution': False,
        'required': [
            'Validate fresh exact catalog, mapping, executable and provider identities.',
            'Compile and load the entire exact role profile set in enforce mode before launch. Refuse any missing profile. Never substitute ix, pix, unconfined or a single-label policy.',
            'Keep NoNewPrivileges true. Qualify required Px transitions with it enabled; a syntax or admission-model test is not kernel enforcement evidence.',
            'Verify systemd and the host administrator use the declared unconfined trusted controller label. The policy does not exclude host root.',
            'Verify workload STOP/CONT sends are denied to itself, helpers, same-unit siblings, supervisor and every other unit. Verify only the matching supervisor or trusted controller can continue the workload.',
            'Treat trusted shutdown, unexpected continuation, child exit and cleanup failure as observation failure. Do not return a successful observation after interrupted ownership.',
            'Bind the label matrix into candidate/root helper expectations and every effective process check. Current production expectations are unchanged and do not yet accept these labels.',
            'Qualify CHLD, normal runtime self-signals, helper termination, process inspection and directed anonymous Rust launch channels without adding blanket peer grants.',
            'Retain all original syscall, file, mapping, mount, identity and raw-map equality controls. This output provides no stable-observation acceptance or production authority.'
        ],
        'base_requirements': json.loads(normal['live-verification-requirements.json'])['mandatory'],
        'documentation': ['https://manpages.ubuntu.com/manpages/noble/man5/apparmor.d.5.html']}
    files = {'apparmor.profile': ('\n'.join(profiles) + '\n').encode(),
             'role-matrix.json': record(matrix),
             'unit-properties.json': record({'schema': 1, 'scope': VERSION,
                                             'qualification_only': True, 'units': properties}),
             'live-verification-requirements.json': record(requirements),
             'validation.json': record({'schema': 1, 'status': 'QUALIFICATION_RENDER_ONLY',
                                        'normal_manifest_sha256': binding['normal_manifest_sha256'],
                                        'profile_count': len(units) * 2, 'identity_sha256': identity,
                                        'kernel_policy_qualified': False, 'g35_accepted': False,
                                        'production_qualified': False}),
             'base-FILE_HASHES.json': normal['FILE_HASHES.json']}
    files['FILE_HASHES.json'] = record({name: {'bytes': len(raw),
                                            'sha256': hashlib.sha256(raw).hexdigest()}
                                      for name, raw in sorted(files.items())})
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--qualification-only', action='store_true', required=True)
    for name in ('catalog', 'mapping', 'request', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--units', nargs='+', required=True)
    args = parser.parse_args()
    try:
        files = generate(args.catalog.read_bytes(), base.decode(args.mapping.read_bytes()),
                         base.decode(args.request.read_bytes()), args.units,
                         qualification_only=args.qualification_only)
        args.output.mkdir(parents=False, exist_ok=False)
        for name, raw in files.items():
            with (args.output / name).open('xb') as stream:
                stream.write(raw)
    except (base.Invalid, OSError, TypeError, KeyError) as error:
        parser.exit(2, 'Qualification policy rendering failed: ' + str(error) + '\n')
    print(json.dumps({'status': 'QUALIFICATION_RENDER_ONLY', 'files': len(files),
                      'output': str(args.output), 'native_execution': False}))


if __name__ == '__main__':
    main()
