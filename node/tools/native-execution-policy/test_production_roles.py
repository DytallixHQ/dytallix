import copy
import hashlib
import json
import unittest

import production_roles as policy
import render
from test_render import fixture
from test_qualification_roles import parsed_profiles, signal_rules


class ProductionRoleTests(unittest.TestCase):
    def inputs(self):
        raw, mapping, request = fixture()
        request['network']['launch_channel'] = render.LAUNCH_CHANNEL
        units = [{'unit': 'node' + str(index), 'uid': uid, 'gid': uid}
                 for index, uid in enumerate(request['service_uids'])]
        return raw, mapping, request, units

    def output(self):
        return policy.generate(*self.inputs(), candidate_only=True)

    def test_production_catalog_carries_its_firewall(self):
        from test_render import production_fixture
        raw, mapping, request, transport = production_fixture()
        request['network']['launch_channel'] = render.LAUNCH_CHANNEL
        units = [{'unit': 'node' + str(index), 'uid': uid, 'gid': uid}
                 for index, uid in enumerate(request['service_uids'])]
        files = policy.generate(raw, mapping, request, units, candidate_only=True, transport=transport)
        self.assertIn('base-host-firewall.nft', files)
        self.assertIn('base-host-firewall.nft', json.loads(files['FILE_HASHES.json']))
        admission = json.loads(files['admission-node0.json'])
        self.assertIn('http_adapter', {row['role'] for row in admission['roles']})
        with self.assertRaises(render.Invalid):
            policy.generate(raw, mapping, request, units, candidate_only=True)

    def test_explicit_inputs_and_determinism(self):
        raw, mapping, request, units = self.inputs()
        with self.assertRaises(render.Invalid):
            policy.generate(raw, mapping, request, units)
        for invalid in ([], units + units[:1], [{**units[0], 'uid': 0}],
                        [{**units[0], 'extra': 1}], [{**units[0], 'unit': '../x'}]):
            with self.subTest(invalid=invalid), self.assertRaises(render.Invalid):
                policy.generate(raw, mapping, request, invalid, candidate_only=True)
        self.assertEqual(self.output(), policy.generate(raw, mapping, request,
                                                       list(reversed(units)), candidate_only=True))

    def test_four_profiles_and_bound_admission(self):
        raw, _, _, _ = self.inputs()
        output = self.output()
        matrix = json.loads(output['role-matrix.json'])
        catalog = json.loads(raw)
        profiles = parsed_profiles(output['apparmor.profile'])
        self.assertEqual(len(profiles), 16)
        self.assertEqual(matrix['identity_sha256'],
                         hashlib.sha256(render.canonical(matrix['binding'])).hexdigest())
        for unit in matrix['units']:
            labels = {name: unit[name + '_label'] for name in policy.ROLE_ORDER}
            self.assertEqual(len(set(labels.values())), 4)
            components = {name: labels['supervisor'].removesuffix('-supervisor') + '-' + name
                          for name in policy.ROLE_ORDER}
            self.assertTrue(set(components.values()) <= set(profiles))
            admission = json.loads(output['admission-' + unit['unit'] + '.json'])
            self.assertEqual(admission['schema'], 2)
            self.assertEqual(admission['policy_identity_sha256'], matrix['identity_sha256'])
            self.assertEqual(admission['catalog_sha512'], hashlib.sha512(raw).hexdigest())
            self.assertEqual({(r['role'], r['member_id']) for r in admission['roles']},
                             {(r['role'], r['member_id']) for r in catalog['roles']})
            for row in admission['roles']:
                self.assertEqual(row['label'], labels[policy.role_of(row['role'])])
            self.assertEqual((admission['no_new_privileges'], admission['seccomp'],
                              admission['mount_namespace']), (1, 2, 'inherit-supervisor'))
            self.assertEqual(set(json.loads(output['unit-properties.json'])['units'][
                int(unit['unit'][4:])]['required_enforce_profiles']), set(components.values()))

    def test_only_directed_exec_and_stop_cont(self):
        output = self.output()
        matrix = json.loads(output['role-matrix.json'])
        profiles = parsed_profiles(output['apparmor.profile'])
        for unit in matrix['units']:
            labels = {name: unit[name + '_label'] for name in policy.ROLE_ORDER}
            components = {name: labels['supervisor'].removesuffix('-supervisor') + '-' + name
                          for name in policy.ROLE_ORDER}
            expected = {(components['supervisor'], labels['application-owner']),
                        (components['supervisor'], labels['workload']),
                        (components['application-owner'], labels['helper']),
                        (components['supervisor'], components['helper'] + '//&' + components['supervisor'])}
            actual_exec = {(entry['source_profile'], entry['target_label'])
                           for entry in matrix['exec'] if entry['mode'] == 'Px-stack'
                           and entry['source_profile'] in components.values()}
            self.assertEqual(actual_exec, expected)
            self.assertFalse(any(entry['source_profile'] == components['helper']
                                 for entry in matrix['exec']))
            supervisor_signals = signal_rules(profiles[components['supervisor']])
            helper_signals = signal_rules(profiles[components['helper']])
            self.assertTrue(any(action == 'send' and 'stop' in names and peer == labels['application-owner']
                                for action, names, peer in supervisor_signals))
            self.assertTrue(any(action == 'send' and 'stop' in names and peer == labels['workload']
                                for action, names, peer in supervisor_signals))
            self.assertTrue(any(action == 'receive' and 'stop' in names and peer == labels['application-owner']
                                for action, names, peer in helper_signals))
            self.assertFalse(any(action == 'receive' and 'stop' in names and peer == labels['supervisor']
                                 for action, names, peer in helper_signals))
            for role in ('workload', 'helper'):
                self.assertIn('deny signal (send) set=(stop, cont),', profiles[components[role]])
        self.assertNotIn('change_profile ', output['apparmor.profile'].decode())

    def test_observation_passes_the_stacked_ptrace_check(self):
        # The kernel checks ptrace against every profile of both labels
        # (AppArmor xcheck_labels): each profile of the reader needs read to
        # the target's label, each profile of the target needs readby from
        # the reader's label. H4 found S unable to open its child's
        # /proc/PID/exe because the S component of A//&S lacked readby S.
        output = self.output()
        profiles = parsed_profiles(output['apparmor.profile'])
        matrix = json.loads(output['role-matrix.json'])

        def allows(profile, access, peer):
            for line in profiles[profile]:
                if line.startswith('ptrace (') and line.endswith(' peer=' + peer + ','):
                    if access in line[len('ptrace ('):line.index(')')].replace(' ', '').split(','):
                        return True
            return False

        for unit in matrix['units']:
            labels = {name: unit[name + '_label'] for name in policy.ROLE_ORDER}
            for reader, target in (('supervisor', 'application-owner'), ('supervisor', 'workload'),
                                   ('application-owner', 'helper')):
                for component in labels[reader].split('//&'):
                    self.assertTrue(allows(component, 'read', labels[target]),
                                    f'{component} cannot read {labels[target]}')
                for component in labels[target].split('//&'):
                    self.assertTrue(allows(component, 'readby', labels[reader]),
                                    f'{component} is not readable by {labels[reader]}')
            # No new observation edge: nothing may read the supervisor or be
            # read by the workload or helper.
            for component in labels['supervisor'].split('//&'):
                for peer in (labels['workload'], labels['helper'], labels['application-owner']):
                    self.assertFalse(allows(component, 'readby', peer) and
                                     all(allows(c, 'read', labels['supervisor']) for c in peer.split('//&')),
                                     f'{peer} can read the supervisor')

    def test_preserved_base_controls_and_hashes(self):
        raw, mapping, request, _ = self.inputs()
        base_output = render.render(raw, mapping, request)
        output = self.output()
        profiles = parsed_profiles(output['apparmor.profile'])
        for lines in profiles.values():
            self.assertIn('owner /proc/[0-9]*/fdinfo/8 r,', lines)
            self.assertIn('owner /proc/[0-9]*/ns/mnt r,', lines)
            self.assertIn('deny ptrace (trace, tracedby),', lines)
            self.assertIn('deny /proc/**/mem w,', lines)
        normal = json.loads(base_output['unit-properties.json'])['properties']
        for unit in json.loads(output['unit-properties.json'])['units']:
            actual = copy.deepcopy(unit['properties'])
            actual['AppArmorProfile'] = normal['AppArmorProfile']
            actual.pop('User')
            actual.pop('Group')
            self.assertEqual(actual, normal)
        for name, content in base_output.items():
            self.assertEqual(output['base-' + name], content)
        hashes = json.loads(output['FILE_HASHES.json'])
        for name, content in output.items():
            if name != 'FILE_HASHES.json':
                self.assertEqual(hashes[name], {'bytes': len(content),
                                                'sha256': hashlib.sha256(content).hexdigest()})
        self.assertFalse(json.loads(output['validation.json'])['production_qualified'])

    def test_launch_socket_peers_are_exact_and_unit_local(self):
        matrix = json.loads(self.output()['role-matrix.json'])
        for unit in matrix['units']:
            labels = {name: unit[name + '_label'] for name in policy.ROLE_ORDER}
            components = {name: labels['supervisor'].removesuffix('-supervisor') + '-' + name
                          for name in policy.ROLE_ORDER}
            expected = {
                'supervisor': {labels['supervisor'], labels['application-owner'],
                               labels['workload'], labels['helper'],
                               components['helper'], components['application-owner'],
                               components['workload']},
                'application-owner': {labels['application-owner'],
                                      labels['supervisor'], labels['helper'],
                                      components['application-owner'], components['helper']},
                'workload': {labels['workload'], labels['supervisor']},
                'helper': {labels['helper'], labels['application-owner'],
                           labels['supervisor'], components['application-owner']},
            }
            for role in policy.ROLE_ORDER:
                actual = {row['peer'] for row in matrix['launch_channel']
                          if row['source_profile'] == components[role]}
                self.assertEqual(actual, expected[role])
                self.assertTrue(all(row['type'] == 'seqpacket' and
                                    row['address'] == 'none' and
                                    row['peer_address'] == 'none'
                                    for row in matrix['launch_channel']
                                    if row['source_profile'] == components[role]))

    def test_every_stack_component_names_the_others_for_the_launch_channel(self):
        # AppArmor revalidates an inherited AF_UNIX socket at exec against
        # each profile of the new stack, with the other components as bare
        # peer labels. H4: S lacked the bare W, so S//&W children (bridge,
        # engine) lost their launch channel and exited.
        matrix = json.loads(self.output()['role-matrix.json'])
        for unit in matrix['units']:
            labels = {name: unit[name + '_label'] for name in policy.ROLE_ORDER}
            for role in ('application-owner', 'workload', 'helper'):
                stack = labels[role].split('//&')
                for component in stack:
                    named = {row['peer'] for row in matrix['launch_channel']
                             if row['source_profile'] == component}
                    for other in stack:
                        if other != component:
                            self.assertIn(other, named, f'{component} does not name {other} ({role})')

    def test_profile_inventory_rejects_missing_or_duplicates(self):
        matrix = json.loads(self.output()['role-matrix.json'])
        labels = [label for unit in matrix['units']
                  for label in unit['required_enforce_profiles']]
        self.assertTrue(policy.require_profiles(matrix, labels))
        with self.assertRaises(render.Invalid):
            policy.require_profiles(matrix, labels[:-1])
        with self.assertRaises(render.Invalid):
            policy.require_profiles(matrix, labels + labels[:1])


if __name__ == '__main__':
    unittest.main()
