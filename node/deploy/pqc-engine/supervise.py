#!/usr/bin/env python3
"""Supervise an existing disposable PQC engine home. Production is forbidden."""
import argparse
import base64
import contextlib
import fcntl
# Use CPython hash implementations. Never fall back to OpenSSL-backed hashlib.
try:
    from _sha2 import sha256, sha512
except ModuleNotFoundError:
    from _sha256 import sha256
    from _sha512 import sha512
import json
import os
import platform
import re
from pathlib import Path
import signal
import socket
import stat
import subprocess
import sys
import time
import tomllib

PROFILE = 'dytallix-pqc-loopback-v1'
MAX_INPUT = 4 * 1024 * 1024
IPC_PROFILE = 'dytallix-pqc-unix-v1'
MAX_EXECUTABLE = 256 * 1024 * 1024


def candidate_inputs(row):
    exact(row, 'config manifest')
    config = pinned_file(row['config'], max_bytes=65536)
    settings = json_file(config)
    exact(settings, 'manifest_path max_manifest_bytes max_executable_bytes')
    for field, maximum in (('max_manifest_bytes', MAX_INPUT), ('max_executable_bytes', MAX_EXECUTABLE)):
        if type(settings[field]) is not int or not 0 < settings[field] <= maximum:
            fail('Candidate resource bound invalid')
    manifest = pinned_file(row['manifest'], max_bytes=settings['max_manifest_bytes'])
    if config.stat().st_mode & 0o6000 or manifest.stat().st_mode & 0o6000:
        fail('Candidate input has unsafe permissions')
    if settings['manifest_path'] != str(manifest): fail('Candidate manifest path differs from pinned input')
    return {'config':config, 'manifest':manifest, 'settings':settings}


def check_candidate(candidate, app, app_config, native, chain_id):
    source = raw_file(candidate['manifest'])
    manifest = json_file(candidate['manifest'])
    fields = 'schema chain_id app_genesis_sha256 target consensus_stdio migration_registry_sha256'
    exact(manifest, fields)
    exact(manifest['target'], 'os arch')
    exact(manifest['consensus_stdio'], 'bytes sha256 sha512')
    canonical = {key:manifest[key] for key in fields.split()}
    canonical['target'] = {key:manifest['target'][key] for key in ('os','arch')}
    canonical['consensus_stdio'] = {key:manifest['consensus_stdio'][key] for key in ('bytes','sha256','sha512')}
    if json.dumps(canonical, ensure_ascii=False, separators=(',',':')).encode() != source:
        fail('Candidate manifest is not canonical Rust JSON')
    if type(manifest['schema']) is not int or manifest['schema'] != 1: fail('Unsupported candidate schema')
    for value, size in ((manifest['app_genesis_sha256'],64),(manifest['migration_registry_sha256'],64),(manifest['consensus_stdio']['sha256'],64),(manifest['consensus_stdio']['sha512'],128)):
        if not isinstance(value,str) or re.fullmatch('[0-9a-f]{'+str(size)+'}', value) is None:
            fail('Candidate digest must use exact lowercase hexadecimal')
    if not isinstance(chain_id,str) or re.fullmatch('[A-Za-z0-9_.-]{1,128}',chain_id) is None:
        fail('Candidate chain identity invalid')
    if manifest['chain_id'] != chain_id or app_config.get('chain_id') != chain_id:
        fail('Candidate chain identity differs')
    if manifest['app_genesis_sha256'] != digest(native) or app_config.get('app_state_sha256') != manifest['app_genesis_sha256']:
        fail('Candidate application genesis differs')
    target = {'os':{'Darwin':'macos','Linux':'linux'}.get(platform.system()), 'arch':{'arm64':'aarch64','aarch64':'aarch64','x86_64':'x86_64','AMD64':'x86_64'}.get(platform.machine())}
    if None in target.values() or manifest['target'] != target: fail('Candidate target differs from service host')
    executable = manifest['consensus_stdio']
    if type(executable['bytes']) is not int or not 0 < executable['bytes'] <= candidate['settings']['max_executable_bytes']:
        fail('Candidate executable byte bound invalid')
    metadata = app.stat()
    if metadata.st_mode & 0o6000: fail('Candidate executable has unsafe permissions')
    if metadata.st_size != executable['bytes'] or digest(app) != executable['sha256']:
        fail('Candidate executable size or SHA-256 differs')
    h = sha512()
    with app.open('rb') as stream:
        total = 0
        for block in iter(lambda:stream.read(65536),b''):
            total += len(block)
            if total > executable['bytes']: fail('Candidate executable exceeded bound while reading')
            h.update(block)
    if total != executable['bytes'] or h.hexdigest() != executable['sha512']:
        fail('Candidate executable SHA-512 differs')
    # Only the application can authenticate committed handover history and select
    # its expected release and migration registry. Do not substitute root history.


def fail(message):
    raise ValueError(message)


def path_checked(raw, directory=False, private=False):
    if not isinstance(raw, str): fail('Path must be a string')
    p = Path(raw)
    if not p.is_absolute() or str(p) != raw or '..' in p.parts: fail('Clean absolute path required')
    for part in [p, *p.parents]:
        if part.is_symlink(): fail('Symlink paths are forbidden')
    st = p.stat()
    if directory and not stat.S_ISDIR(st.st_mode): fail('Directory required')
    if not directory and not stat.S_ISREG(st.st_mode): fail('Regular file required')
    if private and (st.st_mode & 0o077 or st.st_uid != os.getuid()): fail('Private service-owned path required')
    if not directory and (st.st_nlink != 1 or st.st_mode & 0o022): fail('Multiply linked or writable-by-others file forbidden')
    return p


def raw_file(path, private=True):
    p = path_checked(str(path), private=private)
    if not 0 < p.stat().st_size <= MAX_INPUT: fail('Input size exceeds bound')
    return p.read_bytes()


def json_file(path, private=True):
    def unique(pairs):
        out = {}
        for key, value in pairs:
            if key in out: fail('Duplicate JSON field')
            out[key] = value
        return out
    return json.loads(raw_file(path, private), object_pairs_hook=unique)


def digest(path):
    h = sha256()
    with Path(path).open('rb') as f:
        for block in iter(lambda: f.read(1024*1024), b''): h.update(block)
    return h.hexdigest()


def exact(value, fields):
    if not isinstance(value, dict) or set(value) != set(fields.split()): fail('Unexpected manifest fields')


def pinned_file(row, executable=False, max_bytes=None):
    exact(row, 'path sha256')
    p = path_checked(row['path'], private=not executable)
    if max_bytes is not None and not 0 < p.stat().st_size <= max_bytes: fail('Pinned input size exceeds bound')
    if not isinstance(row['sha256'], str) or len(row['sha256']) != 64 or digest(p) != row['sha256']: fail('Artifact hash mismatch')
    if executable and not os.access(p, os.X_OK): fail('Executable permission required')
    return p


def preflight(manifest):
    if os.getuid() == 0: fail('Service must use a non-root identity')
    fields = 'version mode profile home lock_dir engine bridge app application_config native_genesis engine_config validator_public_key_sha256 startup_seconds stop_seconds'
    combined = type(manifest.get('version')) is int and manifest['version'] == 2
    emergency_present = 'emergency' in manifest
    candidate_present = 'candidate' in manifest
    if emergency_present and not combined: fail('Emergency verifier requires version 2 development root configuration')
    if candidate_present and (not combined or not emergency_present): fail('Candidate requires version 2 root and emergency configuration')
    exact(manifest, fields + (' ipc root' if combined else '') + (' emergency' if emergency_present else '') + (' candidate' if candidate_present else ''))
    if type(manifest['version']) is not int or manifest['version'] not in (1, 2) or manifest['mode'] != 'disposable-loopback' or manifest['profile'] != PROFILE: fail('Only disposable loopback mode is supported; production is NO GO')
    for name in ('startup_seconds', 'stop_seconds'):
        if type(manifest[name]) is not int or not 1 <= manifest[name] <= 60: fail('Invalid bounded process timeout')
    home = path_checked(manifest['home'], directory=True, private=True)
    locks = path_checked(manifest['lock_dir'], directory=True, private=True)
    for relative in ('config','data','abci','appdb','data/cs.wal'):
        path_checked(str(home/relative), directory=True, private=True)
    candidate = candidate_inputs(manifest['candidate']) if candidate_present else None
    binaries = {key: pinned_file(manifest[key], True, candidate['settings']['max_executable_bytes'] if candidate and key == 'app' else None) for key in ('engine','bridge','app')}
    app_config = pinned_file(manifest['application_config'], max_bytes=MAX_INPUT)
    app_settings = json_file(app_config)
    if not isinstance(app_settings,dict): fail('Application configuration must be an object')
    if 'release_handover' in app_settings:
        if not isinstance(app_settings['release_handover'],dict):
            fail('Release handover requires an object; omit the field to disable it')
        if candidate is None: fail('Release handover requires explicit candidate configuration')
    native = pinned_file(manifest['native_genesis'], max_bytes=MAX_INPUT if candidate else None)
    engine_config = pinned_file(manifest['engine_config'])
    if engine_config != home/'config/config.toml': fail('Engine config path differs from home')
    cfg = tomllib.loads(raw_file(engine_config).decode())
    required = {'proxy_app':'unix://'+str(home/'abci/app.sock'),'abci':'socket','db_dir':'data','genesis_file':'config/genesis.json','node_key_file':'config/node_key.json','priv_validator_key_file':'config/priv_validator_key.json','priv_validator_state_file':'data/priv_validator_state.json','priv_validator_laddr':''}
    if any(cfg.get(k) != v for k,v in required.items()): fail('Engine state, key, or ABCI path differs')
    if cfg.get('consensus',{}).get('wal_file') != 'data/cs.wal/wal': fail('Consensus WAL must remain in the node data directory')
    for section in ('p2p','rpc'):
        address=cfg.get(section,{}).get('laddr','')
        if not isinstance(address,str) or not address.startswith('tcp://127.0.0.1:'): fail('Only explicit numeric loopback listeners are allowed')
        try: port=int(address.rsplit(':',1)[1])
        except ValueError: fail('Invalid listener port')
        if not 0 < port <= 65535: fail('Invalid listener port')
    forbidden = {'p2p':{'external_address':'','seeds':'','pex':False,'seed_mode':False},'rpc':{'unsafe':False,'grpc_laddr':'','pprof_laddr':'','tls_cert_file':'','tls_key_file':''},'statesync':{'enable':False},'instrumentation':{'prometheus':False}}
    for section, fields in forbidden.items():
        for key,value in fields.items():
            if cfg.get(section,{}).get(key) != value: fail('Unsupported extra listener, discovery, TLS, or state-sync setting')
    # Refuse symlinks throughout the mutable state before children can open a DB or WAL.
    count=0
    for directory in ('data','appdb'):
        for p in (home/directory).rglob('*'):
            count+=1
            if count > 100000: fail('Disposable state inventory exceeds bound')
            if p.is_symlink(): fail('Mutable state symlink forbidden')
            path_checked(str(p), directory=p.is_dir(), private=True)
    for name in ('genesis.json','node_key.json','pqc_transport.json','priv_validator_key.json'):
        raw_file(home/'config'/name)
    raw_file(home/'data/priv_validator_state.json')
    genesis=json_file(home/'config/genesis.json')
    chain=genesis.get('chain_id','').lower()
    if not chain or 'mainnet' in chain or 'production' in chain: fail('Production chain identity forbidden')
    key=json_file(home/'config/priv_validator_key.json').get('pub_key',{})
    if key.get('type') != 'cometbft/PubKeyMlDsa65': fail('ML-DSA-65 validator public identity required')
    try: public=base64.b64decode(key['value'],validate=True)
    except (KeyError,ValueError): fail('Invalid public identity encoding')
    if len(public)!=1952 or base64.b64encode(public).decode()!=key['value']: fail('Invalid full public identity')
    public_hash=sha256(public).hexdigest()
    if manifest['validator_public_key_sha256'] != public_hash: fail('Validator public identity does not match manifest')
    ipc = root = emergency = None
    if combined:
        ipc = manifest['ipc']
        exact(ipc, 'profile adapter listen')
        if ipc['profile'] != IPC_PROFILE: fail('Unsupported Unix RPC profile')
        binaries['adapter'] = pinned_file(ipc['adapter'], True)
        if ipc['listen'] != cfg['rpc']['laddr'].removeprefix('tcp://'): fail('Adapter listener must match configured numeric loopback RPC')
        rpc_socket = home/'data/rpc.sock'
        if rpc_socket.exists() or rpc_socket.is_symlink(): fail('RPC socket already exists; require checked manual recovery')
        root = manifest['root']
        exact(root, 'config helper policy request engine_genesis release_manifest')
        root = {key: pinned_file(value, key == 'helper') for key, value in root.items()}
        root_config = json_file(root['config'])
        if root_config.get('enabled') is not True or root_config.get('profile') != 'SLH-DSA-SHAKE-256s': fail('Explicit development root authorization required')
        for key in ('helper', 'policy', 'request', 'engine_genesis', 'release_manifest'):
            if root_config.get(key + '_path') != str(root[key]): fail('Root input path differs from pinned manifest')
        if root_config.get('helper_sha256') != digest(root['helper']): fail('Root helper hash differs')
        if root['engine_genesis'] != home/'config/genesis.json': fail('Root must bind the actual engine genesis file')
        for key in ('engine_genesis', 'release_manifest'):
            bound = root_config.get('max_' + key + '_bytes')
            if type(bound) is not int or not 0 < root[key].stat().st_size <= bound <= MAX_INPUT: fail('Root artifact bound invalid')
            if sha512(raw_file(root[key])).hexdigest() != root_config.get(key + '_sha512'): fail('Root artifact digest differs')
    if emergency_present:
        exact(manifest['emergency'], 'config helper')
        emergency = {key: pinned_file(value, key == 'helper', 256 * 1024 * 1024 if key == 'helper' else 65536) for key, value in manifest['emergency'].items()}
        verifier = json_file(emergency['config'])
        exact(verifier, 'helper_path helper_scratch_path helper_sha256 max_helper_bytes max_request_bytes timeout_ms')
        if verifier['helper_path'] != str(emergency['helper']) or verifier['helper_sha256'] != digest(emergency['helper']): fail('Emergency helper differs from pinned manifest')
        for field, maximum in (('max_helper_bytes', 256 * 1024 * 1024), ('max_request_bytes', MAX_INPUT), ('timeout_ms', 60000)):
            if type(verifier[field]) is not int or not 0 < verifier[field] <= maximum: fail('Emergency helper resource bound invalid')
        if not 0 < emergency['helper'].stat().st_size <= verifier['max_helper_bytes']: fail('Emergency helper exceeds byte bound')
        scratch = path_checked(verifier['helper_scratch_path'], directory=True, private=True)
        if scratch == home/'abci' or not scratch.is_relative_to(home/'abci'): fail('Emergency scratch must use a dedicated instance ABCI subdirectory')
        if not os.access(scratch, os.W_OK | os.X_OK): fail('Emergency scratch must permit service writes and traversal')
        emergency['scratch'] = scratch
    if candidate:
        check_candidate(candidate, binaries['app'], app_settings, native, genesis.get('chain_id'))
    sock=home/'abci/app.sock'
    if sock.exists() or sock.is_symlink(): fail('ABCI socket already exists; require checked manual recovery')
    return {'home':home,'locks':locks,'binaries':binaries,'app_config':app_config,'native':native,'socket':sock,'public_hash':public_hash,'ipc':ipc,'root':root,'emergency':emergency,'candidate':candidate}


@contextlib.contextmanager
def exclusive(runtime):
    handles=[]
    try:
        # The common directory must be shared by every cooperating service instance.
        for name in ('home-'+sha256(str(runtime['home']).encode()).hexdigest(), 'signer-'+runtime['public_hash']):
            path=runtime['locks']/(name+'.lock')
            fd=os.open(path,os.O_RDWR|os.O_CREAT|os.O_NOFOLLOW,0o600)
            handle=os.fdopen(fd,'r+')
            handles.append(handle)
            st=os.fstat(fd)
            if not stat.S_ISREG(st.st_mode) or st.st_nlink!=1 or st.st_uid!=os.getuid() or st.st_mode & 0o077: fail('Invalid exclusive lock file')
            try: fcntl.flock(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
            except BlockingIOError: fail('Home or validator identity is already in use')
        yield [h.fileno() for h in handles]
    finally:
        # Do not unlink lock files: replacing an inode can bypass an existing lock.
        for handle in reversed(handles): handle.close()


def stop_group(process, seconds):
    if process is None: return
    try: os.killpg(process.pid,signal.SIGTERM)
    except ProcessLookupError: pass
    try: process.wait(timeout=seconds)
    except subprocess.TimeoutExpired:
        try: os.killpg(process.pid,signal.SIGKILL)
        except ProcessLookupError: pass
        process.wait(timeout=5)
    # A bridge child can survive its parent. Kill any remaining owned group.
    try: os.killpg(process.pid,signal.SIGKILL)
    except ProcessLookupError: pass


def stop_all(processes, seconds):
    errors=[]
    for name, process in processes:
        try: stop_group(process, seconds)
        except Exception as error: errors.append(name + ': ' + type(error).__name__ + ': ' + str(error))
    if errors: fail('Owned process cleanup failed: ' + '; '.join(errors))


def run(manifest):
    runtime=preflight(manifest)
    with exclusive(runtime) as descriptors:
        # Recheck configuration and executable bytes after both locks are held.
        runtime=preflight(manifest)
        bridge=engine=adapter=None
        stopping=False
        def request_stop(signum,frame):
            nonlocal stopping
            stopping=True
        prior={s:signal.signal(s,request_stop) for s in (signal.SIGTERM,signal.SIGINT)}
        child_env={k:v for k,v in os.environ.items() if k not in ('HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy')}
        child_env['NO_PROXY']='127.0.0.1,localhost'
        os.umask(0o077)
        try:
            b=runtime['binaries']
            command=[b['bridge'],'--socket','unix://'+str(runtime['socket']),'--',b['app'],'--config',runtime['app_config'],'--genesis',runtime['native'],'--db',runtime['home']/'appdb']
            if runtime['root']:
                command += ['--development-root-config', runtime['root']['config']]
            if runtime['emergency']:
                command += ['--development-emergency-verifier-config', runtime['emergency']['config']]
            if runtime['candidate']:
                command += ['--development-candidate-config', runtime['candidate']['config']]
            bridge=subprocess.Popen(list(map(str,command)),stdin=subprocess.DEVNULL,start_new_session=True,pass_fds=descriptors,env=child_env)
            deadline=time.monotonic()+manifest['startup_seconds']
            while time.monotonic()<deadline and not stopping:
                if bridge.poll() is not None: fail('Bridge exited before socket readiness')
                if runtime['socket'].exists():
                    st=runtime['socket'].lstat()
                    if not stat.S_ISSOCK(st.st_mode) or st.st_uid!=os.getuid() or st.st_mode & 0o077: fail('Invalid ABCI socket identity or permissions')
                    break
                time.sleep(0.05)
            else:
                if stopping: return 0
                fail('Bridge socket startup timed out')
            engine_command=[str(b['engine']),'start','--home',str(runtime['home']),'--p2p-profile',PROFILE]
            if runtime['ipc']: engine_command += ['--rpc-profile', IPC_PROFILE]
            engine=subprocess.Popen(engine_command,stdin=subprocess.DEVNULL,start_new_session=True,pass_fds=descriptors,env=child_env)
            if runtime['ipc']:
                rpc_socket=runtime['home']/'data/rpc.sock'
                deadline=time.monotonic()+manifest['startup_seconds']
                while time.monotonic()<deadline and not stopping:
                    if engine.poll() is not None or bridge.poll() is not None: fail('Engine or bridge exited before RPC readiness')
                    if rpc_socket.exists():
                        st=rpc_socket.lstat()
                        if not stat.S_ISSOCK(st.st_mode) or st.st_uid!=os.getuid() or stat.S_IMODE(st.st_mode)!=0o600: fail('Invalid RPC socket identity or permissions')
                        break
                    time.sleep(0.05)
                else:
                    if stopping: return 0
                    fail('Engine RPC socket startup timed out')
                adapter=subprocess.Popen([str(b['adapter']), '--profile', 'dytallix-pqc-http-local-v1', '--home', str(runtime['home']), '--listen', runtime['ipc']['listen']], stdin=subprocess.DEVNULL, start_new_session=True, pass_fds=descriptors, env=child_env)
            print(json.dumps({'event':'disposable_engine_process_started','production_qualified':False,'validator_public_key_sha256':runtime['public_hash'],'hash_provider':{'sha256':sha256.__module__,'sha512':sha512.__module__,'openssl_hash_module_loaded':'_hashlib' in sys.modules}}),flush=True)
            while not stopping:
                if bridge.poll() is not None or engine.poll() is not None or (adapter is not None and adapter.poll() is not None): fail('Owned service child exited; stopping all process groups')
                time.sleep(0.1)
            return 0
        finally:
            try:
                stop_all([('adapter',adapter),('engine',engine),('bridge',bridge)], manifest['stop_seconds'])
            finally:
                for s,handler in prior.items(): signal.signal(s,handler)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command',choices=('check','run'))
    parser.add_argument('--manifest',type=Path,required=True)
    parser.add_argument('--systemd-instance', help='Enforce fixed systemd unit paths')
    args=parser.parse_args()
    try:
        manifest=json_file(args.manifest)
        if args.systemd_instance is not None:
            instance=args.systemd_instance
            if not re.fullmatch('[a-z0-9][a-z0-9-]{0,31}',instance): fail('Invalid unit instance')
            home=Path('/var/lib/dytallix-pqc')/instance
            if args.manifest!=Path('/etc/dytallix-pqc')/(instance+'.json') or manifest.get('home')!=str(home) or manifest.get('lock_dir')!='/run/dytallix-pqc': fail('Systemd instance requires fixed home, manifest, and shared lock paths')
            for name in ('engine','bridge','app'):
                if not Path(manifest[name]['path']).is_relative_to('/opt/dytallix-node'): fail('Systemd executable must use protected installation tree')
            if manifest.get('version') == 2:
                for row in (manifest['ipc']['adapter'], manifest['root']['helper']):
                    if not Path(row['path']).is_relative_to('/opt/dytallix-node'): fail('Systemd executable must use protected installation tree')
                for key, row in manifest['root'].items():
                    if key != 'helper' and not Path(row['path']).is_relative_to(home/'config'): fail('Systemd root inputs must use protected instance config directory')
            if 'emergency' in manifest:
                if not Path(manifest['emergency']['helper']['path']).is_relative_to('/opt/dytallix-node'): fail('Systemd emergency helper must use protected installation tree')
                if not Path(manifest['emergency']['config']['path']).is_relative_to(home/'config'): fail('Systemd emergency config must use protected instance config directory')
            if 'candidate' in manifest:
                exact(manifest['candidate'], 'config manifest')
                for row in manifest['candidate'].values():
                    if not Path(row['path']).is_relative_to(home/'config'): fail('Systemd candidate inputs must use protected instance config directory')
            for name in ('application_config','native_genesis','engine_config'):
                if not Path(manifest[name]['path']).is_relative_to(home/'config'): fail('Systemd inputs must use protected instance config directory')
        if args.command=='check':
            runtime=preflight(manifest)
            print(json.dumps({'status':'CONFIGURATION_CHECKED','production_qualified':False,'validator_public_key_sha256':runtime['public_hash']}))
            return 0
        return run(manifest)
    except (OSError,ValueError,KeyError,TypeError) as error:
        print(json.dumps({'status':'REFUSED','reason':str(error),'production_qualified':False}),file=sys.stderr)
        return 1

if __name__=='__main__': raise SystemExit(main())
