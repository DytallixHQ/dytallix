import copy,hashlib,json,subprocess,sys,tempfile,unittest
from pathlib import Path
import render

def fixture():
    roles=sorted(render.ROLES|{'http_adapter'})
    members=[{'id':r,'kind':'executable' if r in roles else 'shared_library','bytes':100+i,
      'sha256':hashlib.sha256(r.encode()).hexdigest(),'sha512':hashlib.sha512(r.encode()).hexdigest()}
      for i,r in enumerate(roles+['provider_libc','provider_loader'])]
    c={'schema':2,'chain_id':'development-policy-fixture','app_genesis_sha256':'a'*64,
       'target':{'os':'linux','arch':'x86_64','abi':'gnu'},'migration_registry_sha256':'b'*64,
       'service_profile':'development-linux-native-service-http-v2','members':members,
       'roles':[{'role':r,'member_id':r,'runtime_profile_id':'all'} for r in roles],
       'runtime_profiles':[{'id':'all','member_ids':[m['id'] for m in members if m['kind']=='shared_library'],
         'mapping_policy':'linux-observed-code-v1','interpreted_member_ids':[]}]}
    raw=json.dumps(c,separators=(',',':')).encode()
    m={'schema':1,'members':[{'id':x['id'],'path':'/opt/dyt-policy-fixture/code/'+x['id']} for x in members]}
    r={'schema':1,'policy_version':render.VERSION,'catalog_sha512':hashlib.sha512(raw).hexdigest(),
       'service_uids':[41001,41002,41003,41004],'writable_roots':['/var/lib/dyt-policy-fixture/state','/run/dyt-policy-fixture/runtime'],
       'readonly_files':['/opt/dyt-policy-fixture/config.json'],'code_aliases':[],
       'devices':['/dev/null','/dev/urandom'],'network':{'mode':'shared-private',
         'namespace_path':'/run/netns/dyt-policy-fixture',
         'sockets':[{'family':'unix','type':'stream'},{'family':'inet','type':'stream'}]}}
    return raw,m,r

class RenderingTests(unittest.TestCase):
    def test_saved_service_reproduces_native_abi4_candidate(self):
        saved=Path(__file__).parent/'testdata/abi4-service'
        out=render.render((saved/'catalog.json').read_bytes(),
                          render.decode((saved/'mapping.json').read_bytes()),
                          render.decode((saved/'request.json').read_bytes()))
        profile=out['apparmor.profile']
        self.assertTrue(profile.startswith(b'abi <abi/4.0>,\n'))
        self.assertEqual(hashlib.sha256(profile).hexdigest(),
            '23415deb12c90f6a23bc30fc5241af853c536ad1736aa33e94c56372774ccf0c')
        self.assertEqual(hashlib.sha256(profile[15:]).hexdigest(),
            'de807b46fa5f2bab81bccd248c10496064a62795442c9fbe6e9a37b3638edbc2')
        self.assertEqual(hashlib.sha256(out['unit-properties.json']).hexdigest(),
            '4d913ded136af4b0ba95957dd54223693ff084388e619f2e6cff3166f396840a')
        requirements=json.loads(out['live-verification-requirements.json'])
        self.assertFalse(requirements['verified'])
        self.assertTrue(any('Before compilation or loading' in item and 'abi/4.0' in item and
            'e510bb8f6788b45e48de2f859a6f94a7b8416cbac5a1051814cfce925fa911bd' in item
            for item in requirements['mandatory']))
        validation=json.loads(out['validation.json'])
        self.assertEqual(validation['status'],'RENDERED_NOT_ACTIVATED')
        self.assertFalse(validation['production_qualified'])
        self.assertFalse(validation['g35_accepted'])

    def test_empty_static_runtime_profile(self):
        raw,m,r=fixture();c=json.loads(raw)
        c['members']=[x for x in c['members'] if x['kind']=='executable']
        c['runtime_profiles'][0]['member_ids']=[]
        ids={x['id'] for x in c['members']};m['members']=[x for x in m['members'] if x['id'] in ids]
        raw=json.dumps(c).encode();r['catalog_sha512']=hashlib.sha512(raw).hexdigest()
        out=render.render(raw,m,r)
        self.assertEqual(len(json.loads(out['validation.json'])['members']),len(render.ROLES)+1)
        self.assertNotIn('provider_libc',out['apparmor.profile'].decode())

    def test_mixed_static_dynamic_runtime_profiles(self):
        raw,m,r=fixture();c=json.loads(raw)
        c['runtime_profiles'].append({'id':'static','member_ids':[],
            'mapping_policy':'linux-observed-code-v1','interpreted_member_ids':[]})
        for role in c['roles']:
            if role['role'] in ['genesis_bootstrap_verifier','control_verifier']:role['runtime_profile_id']='static'
        raw=json.dumps(c).encode();r['catalog_sha512']=hashlib.sha512(raw).hexdigest()
        out=render.render(raw,m,r)
        self.assertIn('/opt/dyt-policy-fixture/code/provider_libc rm,',out['apparmor.profile'].decode())
        self.assertIn('/opt/dyt-policy-fixture/code/genesis_bootstrap_verifier rmix,',out['apparmor.profile'].decode())

    def test_runtime_profile_semantics_rejections(self):
        for mode in ['executable_in_profile','unknown_provider','unused_provider','unused_profile']:
            with self.subTest(mode=mode):
                raw,m,r=fixture();c=json.loads(raw)
                if mode=='executable_in_profile':c['runtime_profiles'][0]['member_ids'].append('service_supervisor')
                if mode=='unknown_provider':c['runtime_profiles'][0]['member_ids'].append('unknown')
                if mode=='unused_provider':c['runtime_profiles'][0]['member_ids'].remove('provider_libc')
                if mode=='unused_profile':c['runtime_profiles'].append({'id':'unused','member_ids':[],'mapping_policy':'linux-observed-code-v1','interpreted_member_ids':[]})
                raw=json.dumps(c).encode();r['catalog_sha512']=hashlib.sha512(raw).hexdigest()
                with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_listener_metadata_read_only(self):
        a=render.render(*fixture())['apparmor.profile'].decode()
        self.assertIn('  /proc/[0-9]*/net/{tcp,tcp6} r,',a)
        self.assertNotIn('owner /proc/[0-9]*/net/',a)
        self.assertNotIn('/net/**',a)
        self.assertNotIn('/net/{tcp,tcp6} rw',a)
        self.assertIn('deny /proc/**/mem w,',a)

    def test_profile_identity_read_only(self):
        a=render.render(*fixture())['apparmor.profile'].decode()
        self.assertIn('  owner /proc/[0-9]*/attr/current r,',a)
        self.assertNotIn('/attr/current rw',a)
        self.assertIn('deny /proc/**/mem w,',a)
        self.assertFalse(any('/mem' in line and not line.strip().startswith('deny ') for line in a.splitlines()))

    def test_no_sockets_is_restrictive(self):
        raw,m,r=fixture();r['network']['sockets']=[];o=render.render(raw,m,r)
        a=o['apparmor.profile'].decode();p=json.loads(o['unit-properties.json'])['properties']
        self.assertNotIn('  network ',a);self.assertNotIn('RestrictAddressFamilies',p)
        for name in ['socket','socketpair']:self.assertIn(name,p['SystemCallFilter'].split())

    def test_exact_readonly_directory(self):
        raw,m,r=fixture();r['readonly_directories']=['/opt/dyt-policy-fixture'];o=render.render(raw,m,r)
        a=o['apparmor.profile'].decode()
        self.assertIn('  /opt/dyt-policy-fixture/ r,',a)
        self.assertNotIn('/opt/dyt-policy-fixture/**',a)
        self.assertIn('/opt/dyt-policy-fixture',json.loads(o['unit-properties.json'])['properties']['ReadOnlyPaths'])
        self.assertEqual(json.loads(o['unit-properties.json'])['properties']['ExecPaths'],sorted(x['path'] for x in m['members']))

    def test_readonly_directory_rejections(self):
        values=[['/'],['/opt/x/'],['/opt/**'],['/opt/a b'],['/opt/../a'],['/opt/x','/opt/x'],
                ['/var/lib/dyt-policy-fixture/state'],['/var/lib/dyt-policy-fixture/state/child'],
                ['/opt/dyt-policy-fixture/config.json'],['/opt/dyt-policy-fixture/code/service_supervisor'],
                ['/opt/dyt-policy-fixture/code/service_supervisor/child']]
        for dirs in values:
            with self.subTest(dirs=dirs):
                raw,m,r=fixture();r['readonly_directories']=dirs
                with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_exact_policy_and_no_authority(self):
        raw,m,r=fixture();out=render.render(raw,m,r);p=json.loads(out['unit-properties.json']);v=json.loads(out['validation.json']);a=out['apparmor.profile'].decode()
        self.assertEqual(p['properties']['ExecPaths'],sorted(x['path'] for x in m['members']))
        self.assertEqual(p['properties']['BindReadOnlyPaths'],p['properties']['ExecPaths'])
        self.assertEqual(p['properties']['NoExecPaths'],['/']+sorted(r['writable_roots']))
        self.assertIn('/opt/dyt-policy-fixture/code/provider_libc rm,',a)
        self.assertIn('/opt/dyt-policy-fixture/code/service_supervisor rmix,',a)
        self.assertIn('deny /proc/**/mem w,',a);self.assertNotIn('#include',a);self.assertNotIn(' ux,',a)
        for key in ['authority_verified','kernel_policy_qualified','g35_accepted']:self.assertFalse(v[key])
        self.assertNotIn('User',p['properties']);self.assertFalse(p['properties']['PrivateNetwork'])
        denied=p['properties']['SystemCallFilter'].split()
        for call in ['memfd_create','ptrace','process_vm_writev','recvmsg','recvmmsg','pidfd_getfd','io_uring_setup']:self.assertIn(call,denied)
        self.assertNotIn('pidfd_open',denied)

    def test_deterministic(self):
        self.assertEqual(render.render(*fixture()),render.render(*fixture()))

    def test_denied_syscalls_return_explicit_eperm(self):
        raw,m,r=fixture()
        expected='~@mount memfd_create ptrace process_vm_writev recvmsg recvmmsg pidfd_getfd io_uring_setup io_uring_enter io_uring_register'
        for sockets in (r['network']['sockets'],[]):
            r['network']['sockets']=sockets
            out=render.render(raw,m,r);p=json.loads(out['unit-properties.json'])['properties']
            self.assertEqual(p['SystemCallErrorNumber'],'EPERM')
            self.assertEqual(p['SystemCallFilter'],expected+(' socket socketpair' if not sockets else ''))
            self.assertNotIn('pidfd_open',p['SystemCallFilter'].split())
            self.assertTrue(p['MemoryDenyWriteExecute'])
            self.assertEqual(p['NoExecPaths'],['/']+p['ReadWritePaths'])
            self.assertIn('deny ptrace (trace, tracedby),',out['apparmor.profile'].decode())

    def test_each_writable_mount_has_explicit_noexec(self):
        raw,m,r=fixture()
        # A readonly parent with a writable child matches the native failure.
        r['readonly_directories']=['/opt/dyt-policy-fixture']
        r['writable_roots']=['/opt/dyt-policy-fixture/data','/var/lib/dyt-policy-fixture/state','/run/dyt-policy-fixture/runtime']
        out=render.render(raw,m,r);p=json.loads(out['unit-properties.json'])['properties']
        self.assertEqual(p['ReadWritePaths'],sorted(r['writable_roots']))
        self.assertEqual(set(p['NoExecPaths'])-{'/'},set(p['ReadWritePaths']))
        self.assertEqual(len(p['NoExecPaths']),len(p['ReadWritePaths'])+1)
        self.assertEqual(p['ExecPaths'],sorted(x['path'] for x in m['members']))
        for code in p['ExecPaths']:
            self.assertFalse(any(render.beneath(code,root) for root in p['ReadWritePaths']))
        a=out['apparmor.profile'].decode()
        self.assertIn('  /opt/dyt-policy-fixture/data/** rwk,',a)
        self.assertNotIn('  /opt/dyt-policy-fixture/data/** rm',a)
        requirements=json.loads(out['live-verification-requirements.json'])
        self.assertFalse(requirements['verified'])
        self.assertTrue(any('every declared writable root' in item and 'before readiness' in item for item in requirements['mandatory']))

    def test_writable_exec_alias_collision_still_rejected(self):
        raw,m,r=fixture()
        r['code_aliases']=[{'member_id':'provider_loader','path':'/var/lib/dyt-policy-fixture/state/loader'}]
        with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_unordered_input_normalization(self):
        raw,m,r=fixture();expected=render.render(raw,m,r)
        m['members'].reverse()
        for key in ['service_uids','writable_roots','devices']:r[key].reverse()
        r['network']['sockets'].reverse()
        self.assertEqual(render.render(raw,m,r),expected)

    def test_hash_manifest(self):
        out=render.render(*fixture());manifest=json.loads(out['FILE_HASHES.json'])
        self.assertEqual(set(manifest),set(out)-{'FILE_HASHES.json'})
        for n,p in manifest.items():self.assertEqual(p,{'bytes':len(out[n]),'sha256':hashlib.sha256(out[n]).hexdigest()})

    def test_private_network(self):
        raw,m,r=fixture();r['network'].update(mode='private',namespace_path=None)
        p=json.loads(render.render(raw,m,r)['unit-properties.json'])['properties']
        self.assertTrue(p['PrivateNetwork']);self.assertNotIn('NetworkNamespacePath',p)

    def test_bound_alias(self):
        raw,m,r=fixture();r['code_aliases']=[{'member_id':'provider_loader','path':'/opt/dyt-policy-fixture/loader'}]
        self.assertIn('/opt/dyt-policy-fixture/loader rm,',render.render(raw,m,r)['apparmor.profile'].decode())

    def test_request_rejections(self):
        mutations=[lambda r:r.pop('readonly_files'),lambda r:r.update(unknown=True),lambda r:r.update(catalog_sha512='0'*128),
          lambda r:r.update(policy_version='production'),lambda r:r.update(service_uids=[0]),lambda r:r.update(service_uids=[True]),
          lambda r:r.update(service_uids=[4,4]),lambda r:r.update(schema=True),lambda r:r.update(writable_roots=['/']),
          lambda r:r.update(writable_roots=['/usr/local']),lambda r:r.update(writable_roots=['/opt/dyt-policy-fixture']),
          lambda r:r.update(writable_roots=['/var/lib/dyt','/var/lib/dyt/nested']),lambda r:r.update(devices=['/dev/mem']),
          lambda r:r['network'].update(mode='host'),lambda r:r['network'].update(extra=True),
          lambda r:r['network'].update(sockets=[{'family':'packet','type':'raw'}]),
          lambda r:r.update(code_aliases=[{'member_id':'absent','path':'/opt/dyt/alias'}]),
          lambda r:r.update(code_aliases=[{'member_id':'provider_loader','path':'/opt/dyt-policy-fixture/code/provider_loader'}])]
        for i,mutate in enumerate(mutations):
            with self.subTest(i=i):
                raw,m,r=fixture();mutate(r)
                with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_unsafe_paths(self):
        for p in ['/opt/x/','/opt/**','/opt/a b','/opt/a\nb','/opt/a:b','/opt/$x','/opt/%n','/opt/../x','/opt/./x','//opt/x','relative','/opt/"x','/opt/{x,y}','/opt/[x]','/proc/self/exe']:
            with self.subTest(path=p):
                raw,m,r=fixture();m['members'][0]['path']=p
                with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_catalog_rejections(self):
        mutations=[lambda c:c['members'][0].update(kind='directory'),lambda c:c['members'][0].update(kind='script'),
          lambda c:c['members'][0].update(bytes=True),lambda c:c['members'].append(copy.deepcopy(c['members'][0])),
          lambda c:c['roles'].pop(),lambda c:c['roles'][0].update(member_id='absent'),
          lambda c:c['runtime_profiles'][0].update(mapping_policy='trust-everything'),
          lambda c:c['runtime_profiles'][0].update(interpreted_member_ids=['service_supervisor']),
          lambda c:c['target'].update(arch='aarch64'),lambda c:c.update(service_profile='production')]
        for i,mutate in enumerate(mutations):
            with self.subTest(i=i):
                raw,m,r=fixture();c=json.loads(raw);mutate(c);raw=json.dumps(c).encode();r['catalog_sha512']=hashlib.sha512(raw).hexdigest()
                with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_mapping_rejections(self):
        for mode in ['missing','duplicate','unknown','same_path']:
            with self.subTest(mode=mode):
                raw,m,r=fixture()
                if mode=='missing':m['members'].pop()
                if mode=='duplicate':m['members'].append(copy.deepcopy(m['members'][0]))
                if mode=='unknown':m['members'][0]['id']='absent'
                if mode=='same_path':m['members'][0]['path']=m['members'][1]['path']
                with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_json_limits(self):
        for raw in [b'{"a":1,"a":2}',b'{"a":NaN}',b'',b'x'*(render.MAX_INPUT+1)]:
            with self.assertRaises(render.Invalid):render.decode(raw)

    def test_cli_no_overwrite_or_invalid_output(self):
        raw,m,r=fixture()
        with tempfile.TemporaryDirectory() as directory:
            d=Path(directory)
            for name,data in [('catalog',raw),('mapping',json.dumps(m).encode()),('request',json.dumps(r).encode())]:(d/name).write_bytes(data)
            cmd=[sys.executable,str(Path(render.__file__)),'--catalog',str(d/'catalog'),'--mapping',str(d/'mapping'),'--request',str(d/'request'),'--output',str(d/'out')]
            first=subprocess.run(cmd,capture_output=True);self.assertEqual(first.returncode,0,first.stderr)
            before={p.name:p.read_bytes() for p in (d/'out').iterdir()}
            self.assertEqual(subprocess.run(cmd,capture_output=True).returncode,2)
            self.assertEqual(before,{p.name:p.read_bytes() for p in (d/'out').iterdir()})
            r['catalog_sha512']='0'*128;(d/'request').write_text(json.dumps(r));cmd[-1]=str(d/'rejected')
            self.assertEqual(subprocess.run(cmd,capture_output=True).returncode,2);self.assertFalse((d/'rejected').exists())

class LaunchChannelTests(unittest.TestCase):
    def enabled(self):
        raw,m,r=fixture();r['network']['launch_channel']=render.LAUNCH_CHANNEL
        return raw,m,r

    def test_explicit_channel_generates_only_scoped_unix_rules(self):
        raw,m,r=self.enabled();out=render.render(raw,m,r)
        name=json.loads(out['validation.json'])['profile_name']
        rules=[line for line in out['apparmor.profile'].decode().splitlines() if line.startswith('  unix ')]
        self.assertEqual(rules,['  unix (create) type=seqpacket,',
            f'  unix (send, receive) type=seqpacket addr=none peer=(addr=none,label={name}),'])
        self.assertNotIn('network unix seqpacket',out['apparmor.profile'].decode())
        self.assertEqual(json.loads(out['normalized-request.json'])['network']['launch_channel'],render.LAUNCH_CHANNEL)
        self.assertTrue(out['apparmor.profile'].startswith(b'abi <abi/4.0>,\n'))

    def test_channel_only_enables_unix_family_with_import_denials(self):
        raw,m,r=self.enabled();r['network']['sockets']=[];out=render.render(raw,m,r)
        props=json.loads(out['unit-properties.json'])['properties']
        self.assertEqual(props['RestrictAddressFamilies'],['AF_UNIX'])
        self.assertTrue(props['NoNewPrivileges'])
        expected='~@mount memfd_create ptrace process_vm_writev recvmsg recvmmsg pidfd_getfd io_uring_setup io_uring_enter io_uring_register'
        self.assertEqual(props['SystemCallFilter'],expected)
        self.assertNotIn('  network ',out['apparmor.profile'].decode())

    def test_unknown_null_or_extended_channel_refused(self):
        for value in (None,False,True,1,[],{},'',render.LAUNCH_CHANNEL+' ',
                      {'kind':render.LAUNCH_CHANNEL},'unix-seqpacket', 'rust-1.88-inet-seqpacket-v1'):
            with self.subTest(value=value):
                raw,m,r=fixture();r['network']['launch_channel']=value
                with self.assertRaises(render.Invalid):render.render(raw,m,r)
        raw,m,r=self.enabled();r['network']['launch_peer']='unconfined'
        with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_general_seqpacket_socket_declarations_refused(self):
        for family in ('unix','inet','inet6'):
            raw,m,r=self.enabled();r['network']['sockets'].append({'family':family,'type':'seqpacket'})
            with self.assertRaises(render.Invalid):render.render(raw,m,r)

    def test_channel_changes_binding_but_preserves_existing_controls(self):
        raw,m,r=fixture();base=render.render(raw,m,r)
        r['network']['launch_channel']=render.LAUNCH_CHANNEL;out=render.render(raw,m,r)
        before=json.loads(base['unit-properties.json']);after=json.loads(out['unit-properties.json'])
        self.assertNotEqual(before['policy_id'],after['policy_id'])
        bp=before['properties'];ap=after['properties'];bp.pop('AppArmorProfile');ap.pop('AppArmorProfile')
        self.assertEqual(bp,ap)
        oldname=before['profile_name'];newname=after['profile_name']
        retained=[line.replace(newname,oldname) for line in out['apparmor.profile'].decode().splitlines(True) if not line.startswith('  unix ')]
        self.assertEqual(''.join(retained).encode(),base['apparmor.profile'])

    def test_normalization_and_duplicate_keys_remain_strict(self):
        raw,m,r=self.enabled();a=render.render(raw,m,r)
        r['network']['sockets'].reverse();r['service_uids'].reverse();m['members'].reverse()
        self.assertEqual(a,render.render(raw,m,r))
        with self.assertRaises(render.Invalid):
            render.decode(b'{"launch_channel":"rust-1.88-unix-seqpacket-v1","launch_channel":null}')

    def test_omission_has_no_channel_grants_or_requirements(self):
        out=render.render(*fixture())
        self.assertNotIn('  unix ',out['apparmor.profile'].decode())
        self.assertNotIn('launch_channel',json.loads(out['normalized-request.json'])['network'])
        self.assertEqual(json.loads(out['live-verification-requirements.json'])['mandatory'],render.LIVE_REQUIREMENTS)
        enabled=render.render(*self.enabled())
        self.assertEqual(json.loads(enabled['live-verification-requirements.json'])['mandatory'],
                         render.LIVE_REQUIREMENTS+render.LAUNCH_CHANNEL_REQUIREMENTS)

if __name__=='__main__':unittest.main()
