"""Disposable process-control tests. Fake programs do not test chain semantics."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('supervise',HERE/'supervise.py')
s=importlib.util.module_from_spec(spec);spec.loader.exec_module(s)

BRIDGE='''import os,signal,socket,sys,time
from pathlib import Path
p=Path(sys.argv[sys.argv.index('--socket')+1][7:]);root=p.parent.parent
sock=socket.socket(socket.AF_UNIX);sock.bind(str(p));os.chmod(p,0o600);sock.listen(1)
(root/'data/bridge.started').write_text('yes')
def stop(a,b):
 with (root/'data/order').open('a') as f:f.write('bridge-stop\\n')
 p.unlink();sys.exit(0)
signal.signal(signal.SIGTERM,stop)
while True:time.sleep(.03)
'''
ENGINE='''import signal,sys,time
from pathlib import Path
root=Path(sys.argv[sys.argv.index('--home')+1]);assert (root/'abci/app.sock').exists()
(root/'data/engine.started').write_text('yes')
def stop(a,b):
 with (root/'data/order').open('a') as f:f.write('engine-stop\\n')
 sys.exit(0)
signal.signal(signal.SIGTERM,stop)
while True:
 if (root/'data/exit-engine').exists():sys.exit(3)
 time.sleep(.03)
'''

class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='dyt-service-')
        self.root=Path(self.temp.name).resolve();self.root.chmod(0o700)
        self.home=self.root/'node';self.home.mkdir(mode=0o700)
        for p in ('config','data','abci','appdb','data/cs.wal'):(self.home/p).mkdir(mode=0o700)
        self.locks=self.root/'locks';self.locks.mkdir(mode=0o700)
        self.processes=[]
        self.write('config/genesis.json',json.dumps({'chain_id':'service-fixture'}))
        self.write('config/node_key.json','{}');self.write('config/pqc_transport.json','{}');self.write('data/priv_validator_state.json','{}')
        import base64
        pub=bytes(1952)
        self.write('config/priv_validator_key.json',json.dumps({'pub_key':{'type':'cometbft/PubKeyMlDsa65','value':base64.b64encode(pub).decode()}}))
        self.config='''proxy_app = "unix://SOCKET"
abci = "socket"
db_dir = "data"
genesis_file = "config/genesis.json"
node_key_file = "config/node_key.json"
priv_validator_key_file = "config/priv_validator_key.json"
priv_validator_state_file = "data/priv_validator_state.json"
priv_validator_laddr = ""
[p2p]
laddr = "tcp://127.0.0.1:30001"
external_address = ""
seeds = ""
pex = false
seed_mode = false
[rpc]
laddr = "tcp://127.0.0.1:30002"
unsafe = false
grpc_laddr = ""
pprof_laddr = ""
tls_cert_file = ""
tls_key_file = ""
[statesync]
enable = false
[instrumentation]
prometheus = false
[consensus]
wal_file = "data/cs.wal/wal"
'''.replace('SOCKET',str(self.home/'abci/app.sock'))
        self.write('config/config.toml',self.config)
        self.write('config/app.json','{}');self.write('config/native.json','{}')
        binaries={}
        for name,code in [('engine',ENGINE),('bridge',BRIDGE),('app','pass\n')]:
            p=self.root/name;p.write_text('#!'+sys.executable+'\n'+code);p.chmod(0o700);binaries[name]={'path':str(p),'sha256':s.digest(p)}
        pin=lambda name:{'path':str(self.home/name),'sha256':s.digest(self.home/name)}
        self.manifest={'version':1,'mode':'disposable-loopback','profile':s.PROFILE,'home':str(self.home),'lock_dir':str(self.locks),**binaries,'application_config':pin('config/app.json'),'native_genesis':pin('config/native.json'),'engine_config':pin('config/config.toml'),'validator_public_key_sha256':hashlib.sha256(pub).hexdigest(),'startup_seconds':3,'stop_seconds':2}
    def write(self,name,value):
        p=self.home/name;p.write_text(value);p.chmod(0o600);return p
    def tearDown(self):
        for p in self.processes:
            if p.poll() is None:p.send_signal(signal.SIGTERM)
            try:p.wait(timeout=8)
            except subprocess.TimeoutExpired:p.kill();p.wait()
            p.stdout.close();p.stderr.close()
        self.temp.cleanup()
    def update_config(self,value):
        p=self.write('config/config.toml',value);self.manifest['engine_config']['sha256']=s.digest(p)
    def wait(self,fn):
        end=time.monotonic()+5
        while time.monotonic()<end:
            if fn():return
            time.sleep(.03)
        self.fail('Condition timed out')
    def start(self):
        p=self.root/'manifest.json';p.write_text(json.dumps(self.manifest));p.chmod(0o600)
        child=subprocess.Popen([sys.executable,'-B',str(HERE/'supervise.py'),'run','--manifest',str(p)],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        self.processes.append(child);return child
    def test_cleanup_attempts_every_owned_group(self):
        a,b,c=object(),object(),object()
        with mock.patch.object(s,'stop_group',side_effect=[OSError('fixture failure'),None,None]) as stop:
            with self.assertRaisesRegex(ValueError,'adapter: OSError'):
                s.stop_all([('adapter',a),('engine',b),('bridge',c)],2)
            self.assertEqual(stop.call_args_list,[mock.call(a,2),mock.call(b,2),mock.call(c,2)])
    def test_builtin_hash_vectors_and_file(self):
        for data in (b'', b'abc', bytes(range(256))*17):
            self.assertEqual(s.sha256(data).hexdigest(),hashlib.sha256(data).hexdigest())
            self.assertEqual(s.sha512(data).hexdigest(),hashlib.sha512(data).hexdigest())
        self.assertEqual(s.digest(self.home/'config/genesis.json'),hashlib.sha256((self.home/'config/genesis.json').read_bytes()).hexdigest())
    def test_supervisor_import_has_no_openssl_hash_provider(self):
        code="import importlib.util,json,sys; spec=importlib.util.spec_from_file_location('service',sys.argv[1]); m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m); print(json.dumps({'sha256':m.sha256.__module__,'sha512':m.sha512.__module__,'openssl_loaded':'_hashlib' in sys.modules}))"
        result=subprocess.run([sys.executable,'-B','-c',code,str(HERE/'supervise.py')],capture_output=True,text=True,check=True)
        row=json.loads(result.stdout);self.assertFalse(row['openssl_loaded']);self.assertIn(row['sha256'],('_sha2','_sha256'));self.assertIn(row['sha512'],('_sha2','_sha512'))
    def test_valid_existing_fixture(self):self.assertEqual(s.preflight(self.manifest)['home'],self.home)
    def test_production_refused(self):
        self.manifest['mode']='production'
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_binary_hash_change_refused(self):
        Path(self.manifest['engine']['path']).write_text('changed')
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_wal_escape_refused(self):
        self.update_config(self.config.replace('data/cs.wal/wal','/tmp/wal'))
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_signing_state_path_escape_refused(self):
        self.update_config(self.config.replace('data/priv_validator_state.json','../state.json'))
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_public_listener_refused(self):
        self.update_config(self.config.replace('127.0.0.1:30001','0.0.0.0:30001'))
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_shared_private_key_permissions_refused(self):
        (self.home/'config/priv_validator_key.json').chmod(0o640)
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_mutable_state_symlink_refused(self):
        (self.home/'data/outside').symlink_to(self.root)
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_stale_socket_refused_without_deletion(self):
        sock=socket.socket(socket.AF_UNIX);path=self.home/'abci/app.sock';sock.bind(str(path))
        try:
            with self.assertRaises(ValueError):s.preflight(self.manifest)
            self.assertTrue(path.exists())
        finally:sock.close()
    def test_missing_signing_state_not_generated(self):
        path=self.home/'data/priv_validator_state.json';path.unlink()
        with self.assertRaises(FileNotFoundError):s.preflight(self.manifest)
        self.assertFalse(path.exists())
    def test_identity_lock_rejects_second_home(self):
        runtime=s.preflight(self.manifest)
        other=dict(runtime,home=self.root/'different-home')
        with s.exclusive(runtime):
            with self.assertRaises(ValueError):
                with s.exclusive(other):pass
    def test_lock_symlink_refused(self):
        runtime=s.preflight(self.manifest)
        (self.locks/('home-'+hashlib.sha256(str(self.home).encode()).hexdigest()+'.lock')).symlink_to(self.home/'config/genesis.json')
        with self.assertRaises(OSError):
            with s.exclusive(runtime):pass
    def test_start_order_and_graceful_stop_order(self):
        p=self.start();self.wait(lambda:(self.home/'data/engine.started').exists())
        p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
        self.assertEqual((self.home/'data/order').read_text().splitlines(),['engine-stop','bridge-stop'])
        self.assertFalse((self.home/'abci/app.sock').exists())
    def test_engine_exit_stops_bridge(self):
        p=self.start();self.wait(lambda:(self.home/'data/engine.started').exists())
        self.write('data/exit-engine','yes');self.assertEqual(p.wait(timeout=8),1)
        self.assertFalse((self.home/'abci/app.sock').exists())
    def test_clean_restart_reuses_existing_state(self):
        before=s.digest(self.home/'data/priv_validator_state.json')
        for _ in range(2):
            (self.home/'data/engine.started').unlink(missing_ok=True)
            p=self.start();self.wait(lambda:(self.home/'data/engine.started').exists())
            p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
        self.assertEqual(s.digest(self.home/'data/priv_validator_state.json'),before)


IPC_ENGINE=ENGINE.replace("(root/'data/engine.started').write_text('yes')", """import os,socket
assert sys.argv[sys.argv.index('--rpc-profile')+1]=='dytallix-pqc-unix-v1'
p=root/'data/rpc.sock';sock=socket.socket(socket.AF_UNIX);sock.bind(str(p));os.chmod(p,0o600);sock.listen(1)
(root/'data/engine.started').write_text('yes')""").replace("sys.exit(0)", "p.unlink();sys.exit(0)")
ADAPTER=ENGINE.replace("assert (root/'abci/app.sock').exists()", "assert (root/'data/rpc.sock').exists()").replace('engine.started','adapter.started').replace('engine-stop','adapter-stop').replace('exit-engine','exit-adapter')

class CombinedServiceTests(ServiceTests):
    def setUp(self):
        super().setUp()
        for name,code in [('engine',IPC_ENGINE),('adapter',ADAPTER),('helper','pass\n')]:
            p=self.root/name;p.write_text('#!'+sys.executable+'\n'+code);p.chmod(0o700)
        self.manifest['engine']['sha256']=s.digest(self.root/'engine')
        pin=lambda p:{'path':str(p),'sha256':s.digest(p)}
        self.manifest['version']=2
        self.manifest['ipc']={'profile':s.IPC_PROFILE,'adapter':pin(self.root/'adapter'),'listen':'127.0.0.1:30002'}
        paths={'helper':self.root/'helper','policy':self.write('config/policy.json','{}'),'request':self.write('config/request.json','{}'),'engine_genesis':self.home/'config/genesis.json','release_manifest':self.write('config/release.json','{}')}
        config={'enabled':True,'profile':'SLH-DSA-SHAKE-256s','helper_sha256':s.digest(paths['helper']),**{k+'_path':str(v) for k,v in paths.items()}}
        for key in ('engine_genesis','release_manifest'):
            config[key+'_sha512']=hashlib.sha512(paths[key].read_bytes()).hexdigest();config['max_'+key+'_bytes']=1024
        paths['config']=self.write('config/root.json',json.dumps(config))
        self.manifest['root']={k:pin(v) for k,v in paths.items()}
    def test_start_order_and_graceful_stop_order(self):
        p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists())
        p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
        self.assertEqual((self.home/'data/order').read_text().splitlines(),['adapter-stop','engine-stop','bridge-stop'])
    def test_adapter_exit_stops_all_children(self):
        p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists())
        self.write('data/exit-adapter','yes');self.assertEqual(p.wait(timeout=8),1)
        self.assertFalse((self.home/'abci/app.sock').exists());self.assertFalse((self.home/'data/rpc.sock').exists())
    def test_root_config_forwarded_to_application(self):
        bridge=Path(self.manifest['bridge']['path'])
        bridge.write_text(bridge.read_text().replace("p=Path(sys.argv", "assert '--development-root-config' in sys.argv\np=Path(sys.argv"));self.manifest['bridge']['sha256']=s.digest(bridge)
        p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists());p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
    def enable_emergency(self):
        scratch=self.home/'abci/emergency-helper';scratch.mkdir(mode=0o700)
        helper=self.root/'helper'
        self.verifier={'helper_path':str(helper),'helper_scratch_path':str(scratch),'helper_sha256':s.digest(helper),'max_helper_bytes':1048576,'max_request_bytes':65536,'timeout_ms':1000}
        self.save_verifier()
        self.manifest['emergency']['helper']={'path':str(helper),'sha256':s.digest(helper)}
    def save_verifier(self, raw=None):
        p=self.write('config/emergency.json',json.dumps(self.verifier) if raw is None else raw)
        self.manifest.setdefault('emergency',{})['config']={'path':str(p),'sha256':s.digest(p)}
    def test_emergency_absence_does_not_add_argument(self):
        bridge=Path(self.manifest['bridge']['path'])
        bridge.write_text(bridge.read_text().replace("p=Path(sys.argv", "assert '--development-emergency-verifier-config' not in sys.argv\np=Path(sys.argv"));self.manifest['bridge']['sha256']=s.digest(bridge)
        self.assertIsNone(s.preflight(self.manifest)['emergency'])
        p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists());p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
    def test_emergency_exact_arguments_and_restart(self):
        self.enable_emergency()
        bridge=Path(self.manifest['bridge']['path'])
        checks="assert sys.argv.count('--development-emergency-verifier-config') == 1\nassert sys.argv[sys.argv.index('--development-emergency-verifier-config')+1] == "+repr(self.manifest['emergency']['config']['path'])+"\nassert sys.argv[sys.argv.index('--development-root-config')+1] == "+repr(self.manifest['root']['config']['path'])+"\n"
        bridge.write_text(bridge.read_text().replace("p=Path(sys.argv",checks+"p=Path(sys.argv"));self.manifest['bridge']['sha256']=s.digest(bridge)
        for _ in range(2):
            (self.home/'data/adapter.started').unlink(missing_ok=True)
            p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists());p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
    def test_emergency_requires_root_profile(self):
        self.enable_emergency();self.manifest['version']=1
        del self.manifest['root'];del self.manifest['ipc']
        with self.assertRaisesRegex(ValueError,'requires version 2'):s.preflight(self.manifest)
    def test_emergency_null_and_unknown_fields_refused(self):
        self.enable_emergency()
        for value in (None,{},dict(self.manifest['emergency'],unknown=True)):
            with self.subTest(value=value):
                candidate=copy.deepcopy(self.manifest);candidate['emergency']=value
                with self.assertRaises(ValueError):s.preflight(candidate)
    def test_emergency_config_drift_refused_before_start(self):
        self.enable_emergency();self.write('config/emergency.json','{}')
        p=self.start();self.assertEqual(p.wait(timeout=8),1)
        self.assertFalse((self.home/'data/bridge.started').exists())
    def test_emergency_config_bounds_and_schema(self):
        self.enable_emergency()
        for raw in ('{}','null','{"helper_path":"x","helper_path":"y"}',json.dumps(dict(self.verifier,unknown=True)),json.dumps(self.verifier)+' '*65536):
            with self.subTest(raw_length=len(raw)):
                self.save_verifier(raw)
                with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_emergency_resource_bounds_refused(self):
        self.enable_emergency();original=copy.deepcopy(self.verifier)
        for key,maximum in (('max_helper_bytes',256*1024*1024),('max_request_bytes',s.MAX_INPUT),('timeout_ms',60000)):
            for value in (0,-1,True,'1',maximum+1):
                with self.subTest(key=key,value=value):
                    self.verifier=dict(original,**{key:value});self.save_verifier()
                    with self.assertRaisesRegex(ValueError,'resource bound'):s.preflight(self.manifest)
    def test_emergency_helper_binding_and_size(self):
        self.enable_emergency();original=copy.deepcopy(self.verifier)
        for field,value in (('helper_path',str(self.root/'other-helper')),('helper_sha256','0'*64),('max_helper_bytes',1)):
            with self.subTest(field=field):
                self.verifier=dict(original,**{field:value});self.save_verifier()
                with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_emergency_private_scratch_and_path(self):
        self.enable_emergency();scratch=Path(self.verifier['helper_scratch_path'])
        scratch.chmod(0o750)
        with self.assertRaisesRegex(ValueError,'Private'):s.preflight(self.manifest)
        scratch.chmod(0o700)
        for value in (str(self.home/'abci'),str(self.root),str(scratch)+'/../emergency-helper','relative'):
            with self.subTest(value=value):
                self.verifier['helper_scratch_path']=value;self.save_verifier()
                with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_emergency_scratch_symlink_refused(self):
        self.enable_emergency();link=self.home/'abci/link';link.symlink_to(Path(self.verifier['helper_scratch_path']))
        self.verifier['helper_scratch_path']=str(link);self.save_verifier()
        with self.assertRaisesRegex(ValueError,'Symlink'):s.preflight(self.manifest)
    def test_emergency_systemd_installation_paths(self):
        import contextlib, io
        self.enable_emergency();manifest=copy.deepcopy(self.manifest)
        manifest['home']='/var/lib/dytallix-pqc/fixture';manifest['lock_dir']='/run/dytallix-pqc'
        for key in ('engine','bridge','app'):manifest[key]['path']='/opt/dytallix-node/'+key
        manifest['ipc']['adapter']['path']='/opt/dytallix-node/adapter'
        for key,row in manifest['root'].items():row['path']='/opt/dytallix-node/helper' if key=='helper' else manifest['home']+'/config/'+key
        for key in ('application_config','native_genesis','engine_config'):manifest[key]['path']=manifest['home']+'/config/'+key
        manifest['emergency']['helper']['path']='/opt/dytallix-node/helper'
        manifest['emergency']['config']['path']=manifest['home']+'/config/emergency.json'
        for key,bad in ((None,None),('helper','/tmp/helper'),('config','/tmp/emergency.json')):
            with self.subTest(key=key):
                candidate=copy.deepcopy(manifest)
                if key:candidate['emergency'][key]['path']=bad
                with mock.patch.object(sys,'argv',['supervise','check','--manifest','/etc/dytallix-pqc/fixture.json','--systemd-instance','fixture']),mock.patch.object(s,'json_file',return_value=candidate),mock.patch.object(s,'preflight',return_value={'public_hash':'fixture'}) as preflight,contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(s.main(),1 if key else 0)
                    self.assertEqual(preflight.call_count,0 if key else 1)
    def test_emergency_private_config_required(self):
        self.enable_emergency();Path(self.manifest['emergency']['config']['path']).chmod(0o640)
        with self.assertRaisesRegex(ValueError,'Private'):s.preflight(self.manifest)
    def enable_candidate(self):
        self.enable_emergency()
        self.candidate_settings={'manifest_path':str(self.home/'config/candidate-manifest.json'),'max_manifest_bytes':4096,'max_executable_bytes':1048576}
        app=Path(self.manifest['app']['path'])
        self.candidate_manifest={'schema':1,'chain_id':'service-fixture','app_genesis_sha256':s.digest(self.home/'config/native.json'),'target':{'os':{'Darwin':'macos','Linux':'linux'}[s.platform.system()],'arch':{'arm64':'aarch64','aarch64':'aarch64','x86_64':'x86_64','AMD64':'x86_64'}[s.platform.machine()]},'consensus_stdio':{'bytes':app.stat().st_size,'sha256':s.digest(app),'sha512':hashlib.sha512(app.read_bytes()).hexdigest()},'migration_registry_sha256':'a'*64}
        config={'chain_id':'service-fixture','app_state_sha256':self.candidate_manifest['app_genesis_sha256'],'release_handover':{'fixture':True}}
        p=self.write('config/app.json',json.dumps(config));self.manifest['application_config']['sha256']=s.digest(p)
        self.save_candidate()
    def save_candidate(self, manifest_raw=None, config_raw=None):
        a=self.write('config/candidate-manifest.json',json.dumps(self.candidate_manifest,separators=(',',':')) if manifest_raw is None else manifest_raw)
        b=self.write('config/candidate-config.json',json.dumps(self.candidate_settings) if config_raw is None else config_raw)
        self.manifest['candidate']={'config':{'path':str(b),'sha256':s.digest(b)},'manifest':{'path':str(a),'sha256':s.digest(a)}}
    def test_candidate_forwarding_and_restart_do_not_select_root_release(self):
        self.enable_candidate()
        # Original root release is {}; the distinct target remains a local input.
        self.assertNotEqual(self.manifest['root']['release_manifest']['sha256'],self.manifest['candidate']['manifest']['sha256'])
        bridge=Path(self.manifest['bridge']['path'])
        checks="assert sys.argv.count('--development-candidate-config') == 1\nassert sys.argv[sys.argv.index('--development-candidate-config')+1] == "+repr(self.manifest['candidate']['config']['path'])+"\n"
        bridge.write_text(bridge.read_text().replace("p=Path(sys.argv",checks+"p=Path(sys.argv"));self.manifest['bridge']['sha256']=s.digest(bridge)
        for _ in range(2):
            (self.home/'data/adapter.started').unlink(missing_ok=True)
            p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists());p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
    def test_candidate_omission_preserves_nonhandover_development(self):
        self.assertIsNone(s.preflight(self.manifest)['candidate'])
        bridge=Path(self.manifest['bridge']['path'])
        bridge.write_text(bridge.read_text().replace("p=Path(sys.argv", "assert '--development-candidate-config' not in sys.argv\np=Path(sys.argv"));self.manifest['bridge']['sha256']=s.digest(bridge)
        p=self.start();self.wait(lambda:(self.home/'data/adapter.started').exists());p.send_signal(signal.SIGTERM);self.assertEqual(p.wait(timeout=8),0)
    def test_candidate_handover_omission_null_and_downgrade_refused(self):
        self.enable_candidate();original=copy.deepcopy(self.manifest)
        for field,value in [('candidate','omit'),('candidate',None),('candidate',{}),('candidate',dict(original['candidate'],unknown=True)),('emergency','omit'),('emergency',None),('root',None),('version',1)]:
            with self.subTest(field=field,value=value):
                candidate=copy.deepcopy(original)
                if value=='omit':del candidate[field]
                else:candidate[field]=value
                with self.assertRaises(ValueError):s.preflight(candidate)
    def test_candidate_handover_policy_null_and_nonobjects_refused_before_spawn(self):
        self.enable_candidate()
        for present in (True,False):
            if not present:del self.manifest['candidate']
            for value in (None,False,True,[],0,'disabled'):
                with self.subTest(candidate_present=present,value=value):
                    config={'chain_id':'service-fixture','app_state_sha256':self.candidate_manifest['app_genesis_sha256'],'release_handover':value}
                    p=self.write('config/app.json',json.dumps(config));self.manifest['application_config']['sha256']=s.digest(p)
                    with mock.patch.object(s.subprocess,'Popen') as spawn:
                        with self.assertRaisesRegex(ValueError,'requires an object'):s.run(self.manifest)
                        spawn.assert_not_called()

    def test_candidate_config_exact_schema_and_bounds(self):
        self.enable_candidate();original=copy.deepcopy(self.candidate_settings)
        for raw in ('null','{}','{"manifest_path":"a","manifest_path":"b"}',json.dumps(dict(original,unknown=True)),json.dumps(original)+' '*65536):
            with self.subTest(raw_length=len(raw)):
                self.save_candidate(config_raw=raw)
                with self.assertRaises(ValueError):s.preflight(self.manifest)
        for field,maximum in [('max_manifest_bytes',s.MAX_INPUT),('max_executable_bytes',s.MAX_EXECUTABLE)]:
            for value in (0,-1,True,'1',maximum+1):
                with self.subTest(field=field,value=value):
                    self.candidate_settings=dict(original,**{field:value});self.save_candidate()
                    with self.assertRaisesRegex(ValueError,'resource bound'):s.preflight(self.manifest)
    def test_candidate_manifest_canonical_fields_and_binding(self):
        self.enable_candidate();original=copy.deepcopy(self.candidate_manifest)
        for raw in (json.dumps(original),json.dumps(original,separators=(',',':'))+'\n',json.dumps(dict(reversed(list(original.items()))),separators=(',',':')),'null','{}',json.dumps(dict(original,unknown=True)),json.dumps(original).replace('"schema": 1','"schema": 1, "schema": 1')):
            with self.subTest(raw_length=len(raw)):
                self.save_candidate(manifest_raw=raw)
                with self.assertRaises(ValueError):s.preflight(self.manifest)
        changes=[('schema',True),('schema',2),('chain_id','other'),('app_genesis_sha256','0'*64),('migration_registry_sha256','A'*64),('target',dict(original['target'],arch='other')),('target',dict(reversed(list(original['target'].items())))),('consensus_stdio',dict(reversed(list(original['consensus_stdio'].items()))))]
        changes += [('consensus_stdio',dict(original['consensus_stdio'],**{key:value})) for key,value in [('bytes',True),('bytes',0),('bytes',1),('sha256','0'*64),('sha512','0'*128)]]
        for key,value in changes:
            with self.subTest(key=key,value=value):
                self.candidate_manifest=dict(original,**{key:value});self.save_candidate()
                with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_candidate_config_path_and_actual_app_source_binding(self):
        self.enable_candidate();self.candidate_settings['manifest_path']=str(self.home/'config/other.json');self.save_candidate()
        with self.assertRaisesRegex(ValueError,'path differs'):s.preflight(self.manifest)
        self.candidate_settings['manifest_path']=str(self.home/'config/candidate-manifest.json');self.save_candidate()
        for config in ({'chain_id':'other','app_state_sha256':self.candidate_manifest['app_genesis_sha256']},{'chain_id':'service-fixture','app_state_sha256':'0'*64}):
            p=self.write('config/app.json',json.dumps(config));self.manifest['application_config']['sha256']=s.digest(p)
            with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_candidate_bounds_reject_before_hash_or_manifest_read(self):
        self.enable_candidate();self.candidate_settings['max_manifest_bytes']=1;self.save_candidate()
        target=Path(self.manifest['candidate']['manifest']['path']);original_digest=s.digest
        def bounded_digest(path):
            self.assertNotEqual(Path(path),target,'Oversized manifest must not be hashed')
            return original_digest(path)
        with mock.patch.object(s,'digest',side_effect=bounded_digest):
            with self.assertRaisesRegex(ValueError,'size exceeds bound'):s.preflight(self.manifest)
        self.candidate_settings['max_manifest_bytes']=4096;self.candidate_settings['max_executable_bytes']=1;self.save_candidate()
        target=Path(self.manifest['app']['path'])
        with mock.patch.object(s,'digest',side_effect=bounded_digest):
            with self.assertRaisesRegex(ValueError,'size exceeds bound'):s.preflight(self.manifest)
    def test_candidate_protected_paths_and_drift(self):
        self.enable_candidate()
        for key in ('config','manifest'):
            p=Path(self.manifest['candidate'][key]['path']);p.chmod(0o640)
            with self.assertRaisesRegex(ValueError,'Private'):s.preflight(self.manifest)
            p.chmod(0o600);alias=p.with_suffix('.link');alias.symlink_to(p)
            changed=copy.deepcopy(self.manifest);changed['candidate'][key]['path']=str(alias)
            with self.assertRaisesRegex(ValueError,'Symlink'):s.preflight(changed)
            with p.open('a') as f:f.write(' ')
            with self.assertRaisesRegex(ValueError,'hash mismatch'):s.preflight(self.manifest)
            self.save_candidate()
    def test_candidate_systemd_inputs_remain_in_instance_config(self):
        import contextlib, io
        self.enable_candidate();manifest=copy.deepcopy(self.manifest)
        manifest['home']='/var/lib/dytallix-pqc/fixture';manifest['lock_dir']='/run/dytallix-pqc'
        for key in ('engine','bridge','app'):manifest[key]['path']='/opt/dytallix-node/'+key
        manifest['ipc']['adapter']['path']='/opt/dytallix-node/adapter'
        for key,row in manifest['root'].items():row['path']='/opt/dytallix-node/helper' if key=='helper' else manifest['home']+'/config/'+key
        for key in ('application_config','native_genesis','engine_config'):manifest[key]['path']=manifest['home']+'/config/'+key
        manifest['emergency']['helper']['path']='/opt/dytallix-node/helper'
        manifest['emergency']['config']['path']=manifest['home']+'/config/emergency.json'
        for key,row in manifest['candidate'].items():row['path']=manifest['home']+'/config/candidate-'+key+'.json'
        for key in (None,'config','manifest'):
            with self.subTest(key=key):
                candidate=copy.deepcopy(manifest)
                if key:candidate['candidate'][key]['path']='/tmp/candidate.json'
                with mock.patch.object(sys,'argv',['supervise','check','--manifest','/etc/dytallix-pqc/fixture.json','--systemd-instance','fixture']),mock.patch.object(s,'json_file',return_value=candidate),mock.patch.object(s,'preflight',return_value={'public_hash':'fixture'}) as preflight,contextlib.redirect_stdout(io.StringIO()),contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(s.main(),1 if key else 0)
                    self.assertEqual(preflight.call_count,0 if key else 1)
    def test_candidate_private_input_special_permissions_refused(self):
        self.enable_candidate()
        for key in ('config','manifest'):
            p=Path(self.manifest['candidate'][key]['path']);p.chmod(0o4600)
            with self.assertRaisesRegex(ValueError,'unsafe permissions'):s.preflight(self.manifest)
            p.chmod(0o600)

    def test_candidate_application_special_permissions_refused(self):
        self.enable_candidate();Path(self.manifest['app']['path']).chmod(0o4700)
        with self.assertRaisesRegex(ValueError,'unsafe permissions'):s.preflight(self.manifest)

    def test_root_actual_engine_genesis_required(self):
        other=self.write('config/other-genesis.json',(self.home/'config/genesis.json').read_text())
        self.manifest['root']['engine_genesis']={'path':str(other),'sha256':s.digest(other)}
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_root_file_drift_refused(self):
        self.write('config/release.json','changed')
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_wrong_adapter_port_refused(self):
        self.manifest['ipc']['listen']='127.0.0.1:30003'
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_stale_rpc_socket_refused_without_deletion(self):
        sock=socket.socket(socket.AF_UNIX);path=self.home/'data/rpc.sock';sock.bind(str(path))
        try:
            with self.assertRaises(ValueError):s.preflight(self.manifest)
            self.assertTrue(path.exists())
        finally:sock.close()
    def test_unknown_ipc_profile_refused(self):
        self.manifest['ipc']['profile']='legacy'
        with self.assertRaises(ValueError):s.preflight(self.manifest)
    def test_production_chain_refused(self):
        self.write('config/genesis.json','{"chain_id":"production"}')
        with self.assertRaises(ValueError):s.preflight(self.manifest)

if __name__=='__main__':unittest.main(verbosity=2)
