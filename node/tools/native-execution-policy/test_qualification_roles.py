import copy
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import qualification_roles as q
import render
from test_render import fixture


def inputs():
    raw, mapping, request = fixture()
    request['network']['launch_channel'] = render.LAUNCH_CHANNEL
    return raw, mapping, request


def parsed_profiles(raw):
    profiles, current = {}, None
    for line in raw.decode().splitlines():
        if line.startswith('profile '):
            current = line.split()[1]
            profiles[current] = []
        elif line == '}':
            current = None
        elif current:
            profiles[current].append(line.strip())
    return profiles


def signal_rules(lines):
    result = []
    for line in lines:
        match = re.fullmatch(r'signal \((send|receive)\) set=\(([^)]+)\) peer=([^,]+),', line)
        if match:
            result.append((match[1], {s.strip() for s in match[2].split(',')}, match[3]))
    return result


def signal_allowed(profiles, source, target, name):
    # Pure interpretation of the emitted allow/deny subset, not an AppArmor emulator.
    if source in profiles and name in ('stop', 'cont') and \
            'deny signal (send) set=(stop, cont),' in profiles[source]:
        return False
    def grant(label, access, peer):
        if label == 'unconfined':
            return True
        return any(a == access and name in names and p == peer
                   for a, names, p in signal_rules(profiles[label]))
    return grant(source, 'send', target) and grant(target, 'receive', source)


class QualificationRoleTests(unittest.TestCase):
    def output(self, units=None):
        return q.generate(*inputs(), units or ['node0', 'node1', 'node2', 'node3'],
                          qualification_only=True)

    def test_explicit_qualification_boundary(self):
        with self.assertRaisesRegex(render.Invalid, 'qualification-only'):
            q.generate(*inputs(), ['node0'])
        with self.assertRaises(render.Invalid):
            q.generate(*inputs(), ['node0'], qualification_only=1)

    def test_unit_identifier_and_count_rejections(self):
        for units in ([], ['x'] * 2, ['../x'], ['a b'], ['*'], ['x' * 33],
                      ['node' + str(i) for i in range(17)], 'node0'):
            with self.subTest(units=units), self.assertRaises(render.Invalid):
                q.generate(*inputs(), units, qualification_only=True)

    def test_required_exact_channel(self):
        raw, mapping, request = inputs()
        del request['network']['launch_channel']
        with self.assertRaisesRegex(render.Invalid, 'Rust launch'):
            q.generate(raw, mapping, request, ['node0'], qualification_only=True)

    def test_inherited_catalog_validation_remains_required(self):
        raw, mapping, request = inputs()
        request['catalog_sha512'] = '0' * 128
        with self.assertRaisesRegex(render.Invalid, 'Catalog digest'):
            q.generate(raw, mapping, request, ['node0'], qualification_only=True)

    def test_deterministic_order_and_unique_labels(self):
        self.assertEqual(self.output(['node1', 'node0']), self.output(['node0', 'node1']))
        out = self.output()
        profiles = parsed_profiles(out['apparmor.profile'])
        self.assertEqual(len(profiles), 8)
        matrix = json.loads(out['role-matrix.json'])
        self.assertEqual(set(profiles), {u[k] for u in matrix['units']
                                       for k in ('supervisor', 'workload')})
        altered = self.output(['node0', 'node1', 'node2'])
        self.assertTrue(set(profiles).isdisjoint(parsed_profiles(altered['apparmor.profile'])))

    def test_directed_required_exec_and_no_fallback(self):
        out = self.output()
        profiles = parsed_profiles(out['apparmor.profile'])
        matrix = json.loads(out['role-matrix.json'])
        catalog = json.loads(inputs()[0])
        workload = ['/opt/dyt-policy-fixture/code/' + m['id'] for m in catalog['members']
                    if m['kind'] == 'executable' and m['id'] != 'service_supervisor']
        for unit in matrix['units']:
            sup, work = unit['supervisor'], unit['workload']
            for path in workload:
                self.assertIn(path + ' rPx -> ' + work + ',', profiles[sup])
                self.assertNotIn(path + ' rmix,', profiles[sup])
                self.assertIn(path + ' rmix,', profiles[work])
            exec_lines = [line for line in profiles[sup] if ' -> ' in line]
            self.assertEqual(len(exec_lines), len(workload))
        text = out['apparmor.profile'].decode()
        self.assertFalse(re.search(r'\b(?:pix|Pix|pux|PUx|ux|Ux|cix|Cix|cux|CUx)\b', text))
        self.assertNotIn('change_profile ', text)

    def test_workload_cannot_execute_or_map_supervisor_or_alias(self):
        raw, mapping, request = inputs()
        alias = '/opt/dyt-policy-fixture/code/supervisor-alias'
        request['code_aliases'].append({'member_id': 'service_supervisor', 'path': alias})
        out = q.generate(raw, mapping, request, ['node0'], qualification_only=True)
        profiles = parsed_profiles(out['apparmor.profile'])
        unit = json.loads(out['role-matrix.json'])['units'][0]
        for path in ('/opt/dyt-policy-fixture/code/service_supervisor', alias):
            self.assertIn('deny ' + path + ' xm,', profiles[unit['workload']])
            self.assertIn(path + ' r,', profiles[unit['workload']])
            self.assertNotIn(path + ' rmix,', profiles[unit['workload']])
            self.assertIn(path + ' rmix,', profiles[unit['supervisor']])
        self.assertFalse(any(e['source'] == unit['workload'] and e['target'] == unit['supervisor']
                             for e in json.loads(out['role-matrix.json'])['exec']))

    def test_supervisor_workload_executable_role_alias_rejected(self):
        raw, mapping, request = inputs()
        catalog = json.loads(raw)
        for role in catalog['roles']:
            if role['role'] == 'control_verifier':
                role['member_id'] = 'service_supervisor'
        catalog['members'] = [m for m in catalog['members'] if m['id'] != 'control_verifier']
        mapping['members'] = [m for m in mapping['members'] if m['id'] != 'control_verifier']
        raw = json.dumps(catalog).encode()
        request['catalog_sha512'] = hashlib.sha512(raw).hexdigest()
        with self.assertRaisesRegex(render.Invalid, 'cannot serve'):
            q.generate(raw, mapping, request, ['node0'], qualification_only=True)

    def test_complete_stop_cont_pair_matrix(self):
        out = self.output()
        profiles = parsed_profiles(out['apparmor.profile'])
        units = json.loads(out['role-matrix.json'])['units']
        permitted = {(u['supervisor'], u['workload']) for u in units}
        for source in profiles:
            for target in profiles:
                for name in ('stop', 'cont'):
                    self.assertEqual(signal_allowed(profiles, source, target, name),
                                     (source, target) in permitted, (source, target, name))

    def test_workload_explicit_stop_cont_send_deny_has_no_shadowing_allow(self):
        out = self.output()
        profiles = parsed_profiles(out['apparmor.profile'])
        for unit in json.loads(out['role-matrix.json'])['units']:
            lines = profiles[unit['workload']]
            self.assertIn('deny signal (send) set=(stop, cont),', lines)
            self.assertFalse(any(access == 'send' and names & {'stop', 'cont'}
                                 for access, names, _ in signal_rules(lines)))
            self.assertFalse(any(line.startswith('deny signal (receive)') for line in lines))

    def test_trusted_shutdown_exception_and_child_lifecycle(self):
        out = self.output()
        profiles = parsed_profiles(out['apparmor.profile'])
        for unit in json.loads(out['role-matrix.json'])['units']:
            sup, work = unit['supervisor'], unit['workload']
            for target in (sup, work):
                for signal in ('term', 'kill', 'cont'):
                    self.assertTrue(signal_allowed(profiles, 'unconfined', target, signal))
                self.assertFalse(signal_allowed(profiles, 'unconfined', target, 'stop'))
                self.assertTrue(signal_allowed(profiles, target, 'unconfined', 'chld'))
            self.assertTrue(signal_allowed(profiles, work, sup, 'chld'))
            for signal in ('term', 'kill'):
                self.assertTrue(signal_allowed(profiles, sup, work, signal))
            for signal in ('chld', 'term', 'urg', 'rtmin+0'):
                self.assertTrue(signal_allowed(profiles, work, work, signal))

    def test_every_signal_allow_has_explicit_set_and_exact_peer(self):
        profiles = parsed_profiles(self.output()['apparmor.profile'])
        for lines in profiles.values():
            signal_lines = [line for line in lines if line.startswith('signal ')]
            self.assertEqual(len(signal_lines), len(signal_rules(lines)))
            for _, _, peer in signal_rules(lines):
                self.assertIn(peer, set(profiles) | {'unconfined'})
                self.assertFalse(any(c in peer for c in '*?[]{}'))

    def test_ptrace_read_direction_and_no_cross_unit_pairs(self):
        out = self.output()
        matrix = json.loads(out['role-matrix.json'])
        expected = []
        for unit in matrix['units']:
            s, w = unit['supervisor'], unit['workload']
            expected += [(s, 'read, readby', s), (s, 'read', w),
                         (w, 'read, readby', w), (w, 'readby', s)]
        self.assertEqual([(r['source_label'], r['access'], r['peer'])
                          for r in matrix['ptrace']], expected)
        for lines in parsed_profiles(out['apparmor.profile']).values():
            self.assertIn('deny ptrace (trace, tracedby),', lines)
            self.assertFalse(any(line.startswith('ptrace ') and
                                 ('trace' in line.removeprefix('ptrace ') or 'peer=' not in line)
                                 for line in lines))

    def test_launch_channels_exact_same_unit_directed_pairs(self):
        out = self.output()
        matrix = json.loads(out['role-matrix.json'])
        expected = set()
        for unit in matrix['units']:
            s, w = unit['supervisor'], unit['workload']
            expected |= {(s, s), (s, w), (w, s), (w, w)}
        self.assertEqual({(r['source_label'], r['peer']) for r in matrix['launch_channel']}, expected)
        for r in matrix['launch_channel']:
            self.assertEqual((r['type'], r['address'], r['peer_address']), ('seqpacket', 'none', 'none'))
        for lines in parsed_profiles(out['apparmor.profile']).values():
            self.assertEqual(lines.count('unix (create) type=seqpacket,'), 1)
            self.assertEqual(sum(line.startswith('unix (send, receive)') for line in lines), 2)
            self.assertFalse(any(line.startswith('unix ') and
                                 any(word in line for word in ('connect', 'bind', 'listen'))
                                 for line in lines))

    def test_all_other_unit_controls_equal_normal_output(self):
        normal = json.loads(render.render(*inputs())['unit-properties.json'])['properties']
        out = self.output()
        for unit in json.loads(out['unit-properties.json'])['units']:
            props = copy.deepcopy(unit['properties'])
            props['AppArmorProfile'] = normal['AppArmorProfile']
            self.assertEqual(props, normal)
            self.assertTrue(props['NoNewPrivileges'])
            self.assertTrue(props['MemoryDenyWriteExecute'])
            self.assertIn('recvmsg recvmmsg pidfd_getfd', props['SystemCallFilter'])
            self.assertIn('io_uring_setup io_uring_enter io_uring_register', props['SystemCallFilter'])

    def test_data_provider_and_mapping_denials_preserved(self):
        out = self.output()
        for lines in parsed_profiles(out['apparmor.profile']).values():
            self.assertIn('/opt/dyt-policy-fixture/code/provider_libc rm,', lines)
            self.assertIn('/opt/dyt-policy-fixture/code/provider_loader rm,', lines)
            self.assertIn('/opt/dyt-policy-fixture/config.json r,', lines)
            self.assertIn('/var/lib/dyt-policy-fixture/state/** rwk,', lines)
            self.assertIn('deny /proc/**/mem w,', lines)
            self.assertIn('owner /proc/[0-9]*/{stat,maps,status,comm,exe} r,', lines)
            self.assertIn('network inet stream,', lines)
            self.assertNotIn('network,', lines)
            self.assertFalse(any(line.startswith('deny /**') for line in lines))

    def test_missing_profile_refuses_admission_model(self):
        matrix = json.loads(self.output()['role-matrix.json'])
        all_labels = [r[k] for r in matrix['units'] for k in ('supervisor', 'workload')]
        self.assertTrue(q.require_profiles(matrix, all_labels))
        for missing in all_labels:
            with self.subTest(missing=missing), self.assertRaisesRegex(render.Invalid, 'missing'):
                q.require_profiles(matrix, [p for p in all_labels if p != missing])
        with self.assertRaises(render.Invalid):
            q.require_profiles(matrix, all_labels + [all_labels[0]])

    def test_output_explicitly_has_no_native_or_production_acceptance(self):
        out = self.output()
        v = json.loads(out['validation.json'])
        self.assertEqual(v['status'], 'QUALIFICATION_RENDER_ONLY')
        for key in ('kernel_policy_qualified', 'g35_accepted', 'production_qualified'):
            self.assertFalse(v[key])
        self.assertFalse(json.loads(out['role-matrix.json'])['kernel_enforcement_verified'])
        self.assertIn('NoNewPrivileges true', out['live-verification-requirements.json'].decode())

    def test_normal_render_unchanged_and_inputs_not_mutated(self):
        args = inputs()
        before = copy.deepcopy(args)
        normal = render.render(*args)
        q.generate(*args, ['node0'], qualification_only=True)
        self.assertEqual(args, before)
        self.assertEqual(render.render(*args), normal)

    def test_output_hash_manifest(self):
        out = self.output()
        manifest = json.loads(out['FILE_HASHES.json'])
        self.assertEqual(set(manifest), set(out) - {'FILE_HASHES.json'})
        for name, pin in manifest.items():
            self.assertEqual(pin, {'bytes': len(out[name]), 'sha256': hashlib.sha256(out[name]).hexdigest()})

    def test_cli_refuses_missing_qualification_flag_without_output(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'output'
            result = subprocess.run([sys.executable, '-B', str(Path(q.__file__)),
                                     '--catalog', 'unused', '--mapping', 'unused',
                                     '--request', 'unused', '--units', 'node0', '--output', str(target)],
                                    capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 2)
            self.assertFalse(target.exists())


if __name__ == '__main__':
    unittest.main()
