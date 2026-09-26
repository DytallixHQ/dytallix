#!/usr/bin/env python3
"""Render staging policy only. This program never verifies or activates host policy."""
import argparse
import hashlib
import json
import re
from pathlib import Path, PurePosixPath

VERSION = 'native-library-staging-v1'
LAUNCH_CHANNEL = 'rust-1.88-unix-seqpacket-v1'
LAUNCH_CHANNEL_REQUIREMENTS = [
    'Verify native fine-grained AppArmor network/af_unix mediation and the selected ABI before compiling or loading this launch-channel policy. Refuse a missing feature, rule downgrade or unconfined fallback.',
    'Qualify the exact Rust launch channel: sequence-packet creation is type-scoped; send/receive require anonymous local and peer addresses and the same profile. This policy does not prove parent-child identity or limit creation to socketpair. Verify that relationship through the owned launcher.',
    'Keep recvmsg, recvmmsg, pidfd_getfd and io_uring denied during launch success, launch-error reporting and helper READY/request/result/DONE-ACK qualification. No ancillary descriptor-import exception is granted.',
]
ROLES = {'consensus_stdio', 'consensus_bridge', 'consensus_engine',
         'genesis_bootstrap_verifier', 'control_verifier', 'service_supervisor'}
MAX_INPUT = 4 * 1024 * 1024
MAX_RECORDS = 256
LIVE_REQUIREMENTS = [
    'Before compilation or loading, resolve abi/4.0 only from the selected pinned parser include/base root. Verify its root-owned regular file bytes have SHA256 e510bb8f6788b45e48de2f859a6f94a7b8416cbac5a1051814cfce925fa911bd, record the resolved path and digest, and reject a missing or mismatched ABI file without host fallback.',
    'Validate the catalog with the maintained release-runtime validator and trusted expected release inputs.',
    'Verify every code path and alias is a regular ELF file with matching catalog hashes and size; directories are forbidden.',
    'Verify root ownership, no service write permission or writable ACL, stable file identity, and protected parent directories for code, profile, catalog and unit inputs.',
    'Verify code backing content cannot change through another writable mount or file alias; record fs-verity or an explicitly limited staging immutability basis.',
    'Verify effective file-level read-only and execution mounts; check every declared writable root and its backing mounts are writable and noexec before readiness. A parent noexec mount is insufficient evidence.',
    'Verify AppArmor is enabled, the exact owned profile is loaded in enforce mode, and the workload enters it before exec.',
    'Verify required systemd and native seccomp controls are supported and effective; never ignore unavailable controls.',
    'Verify one declared numeric non-root service UID per unit, external harness separation, and declared inherited descriptors only.',
    'For shared-private networking verify the named namespace belongs to the owned anchor and differs from the host namespace.',
    'Verify recvmsg/recvmmsg and io_uring denial is compatible with exact Go/Rust socket paths; never silently relax descriptor-import controls.',
    'Verify root helper executes the immutable catalog path with the qualified handshake; no writable executable snapshot exception.',
    'Qualify inert positive/denial probes and exact workload startup, commitment, receipts, helper behavior, shutdown and restart.',
    'Keep sampled mapping observation, policy prevention, provider source assurance and production approval as separate records.',
]

class Invalid(ValueError):
    pass

def require(ok, message):
    if not ok:
        raise Invalid(message)

def keys(value, expected, label):
    require(type(value) is dict and set(value) == set(expected), label + ': missing or unknown fields')

def array(value, label, empty=False):
    require(type(value) is list and (empty or bool(value)) and len(value) <= MAX_RECORDS, label + ': list bound')
    return value

def identifier(value):
    require(type(value) is str and re.fullmatch(r'[A-Za-z0-9_.-]{1,128}', value), 'Invalid identifier')
    return value

def digest(value, size):
    require(type(value) is str and re.fullmatch('[0-9a-f]{'+str(size)+'}', value), 'Invalid digest')
    return value

def path(value):
    require(type(value) is str and 1 < len(value) <= 1024 and
            re.fullmatch(r'/(?:[A-Za-z0-9_+.-]+/)*[A-Za-z0-9_+.-]+', value), 'Unsafe path syntax')
    require(all(p not in ('.', '..') for p in value.split('/')[1:]), 'Dot path component')
    require(str(PurePosixPath(value)) == value, 'Noncanonical path')
    return value

def beneath(value, root):
    return value == root or value.startswith(root + '/')

def unique(values, label):
    require(len(values) == len(set(values)), label + ': duplicate')

def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True).encode()

def reject_duplicates(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'Duplicate JSON key')
        result[key] = value
    return result

def decode(raw):
    require(type(raw) is bytes and 0 < len(raw) <= MAX_INPUT, 'JSON input byte bound')
    try:
        return json.loads(raw, object_pairs_hook=reject_duplicates,
                          parse_constant=lambda _: (_ for _ in ()).throw(Invalid('Non-finite JSON value')))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise Invalid('Invalid JSON') from error

def validate(catalog_bytes, mapping, request):
    catalog = decode(catalog_bytes)
    request_keys = ['schema','policy_version','catalog_sha512','service_uids','writable_roots',
                    'readonly_files','code_aliases','devices','network']
    if type(request) is dict and 'readonly_directories' in request:
        request_keys.append('readonly_directories')
    keys(request, request_keys, 'request')
    require(type(request['schema']) is int and request['schema'] == 1 and request['policy_version'] == VERSION,
            'Unsupported control policy')
    require(hashlib.sha512(catalog_bytes).hexdigest() == digest(request['catalog_sha512'], 128), 'Catalog digest mismatch')
    keys(catalog, ['schema','chain_id','app_genesis_sha256','target','migration_registry_sha256',
                   'service_profile','members','roles','runtime_profiles'], 'catalog')
    require(type(catalog['schema']) is int and catalog['schema'] == 2, 'Unsupported catalog schema')
    identifier(catalog['chain_id']); digest(catalog['app_genesis_sha256'],64); digest(catalog['migration_registry_sha256'],64)
    require(catalog['target'] == {'os':'linux','arch':'x86_64','abi':'gnu'}, 'Unsupported target')
    require(catalog['service_profile'] in ('development-linux-native-service-v2',
                'development-linux-native-service-http-v2'), 'Only native staging service profiles are supported')
    members = {}
    for member in array(catalog['members'], 'members'):
        keys(member,['id','kind','bytes','sha256','sha512'],'member'); mid=identifier(member['id'])
        require(mid not in members, 'Duplicate member')
        require(member['kind'] in ('executable','shared_library'), 'Directory, script and interpreted members are prohibited')
        require(type(member['bytes']) is int and 0 < member['bytes'] <= 2**34, 'Invalid member byte size')
        digest(member['sha256'],64);digest(member['sha512'],128);members[mid]=member
    unique([(m['bytes'],m['sha256'],m['sha512']) for m in members.values()], 'Member content')
    profiles={}
    for profile in array(catalog['runtime_profiles'],'runtime_profiles'):
        keys(profile,['id','member_ids','mapping_policy','interpreted_member_ids'],'runtime profile')
        pid=identifier(profile['id']);require(pid not in profiles,'Duplicate runtime profile')
        require(profile['mapping_policy']=='linux-observed-code-v1' and profile['interpreted_member_ids']==[],
                'Unsupported mapping or interpreted policy')
        mids=array(profile['member_ids'],'member_ids',True);unique(mids,'Profile member ids')
        require(all(mid in members for mid in mids),'Unknown profile member')
        require(all(members[mid]['kind']=='shared_library' for mid in mids),'Runtime profiles contain shared libraries only')
        profiles[pid]=set(mids)
    required=ROLES | ({'http_adapter'} if '-http-' in catalog['service_profile'] else set())
    roles={};used_profiles=set()
    for role in array(catalog['roles'],'roles'):
        keys(role,['role','member_id','runtime_profile_id'],'role');name=identifier(role['role'])
        mid=identifier(role['member_id']);pid=identifier(role['runtime_profile_id'])
        require(name not in roles and mid in members and pid in profiles,'Invalid role reference')
        require(members[mid]['kind']=='executable','Role must reference an executable member')
        roles[name]=mid;used_profiles.add(pid)
    require(set(roles)==required,'Missing or unknown required roles')
    require(used_profiles==set(profiles),'Unreferenced runtime profile')
    used_members=set(roles.values()) | set().union(*profiles.values())
    require(used_members==set(members),'Unreferenced catalog member')
    require({m for m,v in members.items() if v['kind']=='executable'}==set(roles.values()),'Unassigned executable')
    keys(mapping,['schema','members'],'mapping');require(type(mapping['schema']) is int and mapping['schema']==1,'Unsupported mapping schema')
    paths={};all_paths=[]
    for item in array(mapping['members'],'mapping members'):
        keys(item,['id','path'],'mapping member');mid=identifier(item['id']);p=path(item['path'])
        require(mid in members and mid not in paths,'Duplicate or unknown mapping member');paths[mid]=[p];all_paths.append(p)
    require(set(paths)==set(members),'Incomplete mapping')
    for alias in array(request['code_aliases'],'code aliases',True):
        keys(alias,['member_id','path'],'alias');mid=identifier(alias['member_id']);p=path(alias['path'])
        require(mid in paths,'Unknown alias member');paths[mid].append(p);all_paths.append(p)
    unique(all_paths,'Code path or alias')
    require(not any(beneath(p,r) for p in all_paths for r in ['/proc','/sys','/dev']), 'Code on API filesystem is prohibited')
    writable=[path(p) for p in array(request['writable_roots'],'writable roots')]
    unique(writable,'Writable roots')
    for p in writable:
        require(len(PurePosixPath(p).parts)>=3 and not any(beneath(p,r) for r in ['/proc','/sys','/dev','/usr','/etc','/bin','/sbin','/lib','/lib64']), 'Unsafe writable root')
        require(not any(p != q and beneath(p,q) for q in writable),'Overlapping writable roots')
    require(not any(beneath(p,r) for p in all_paths for r in writable),'Code under writable root')
    readonly=[path(p) for p in array(request['readonly_files'],'readonly files',True)]
    unique(readonly,'Readonly files');require(not set(readonly)&set(all_paths),'Readonly file duplicates code path')
    require(not any(beneath(p,r) for p in readonly for r in writable),'Readonly file under writable root')
    readdirs=[path(p) for p in array(request.get('readonly_directories',[]),'readonly directories',True)]
    unique(readdirs,'Readonly directories')
    require(not set(readdirs)&set(readonly),'Readonly file/directory collision')
    require(not any(beneath(p,r) for p in readdirs for r in writable),'Readonly directory inside writable root')
    require(not any(beneath(p,c) for p in readdirs for c in all_paths),'Readonly directory conflicts with code file')
    uids=array(request['service_uids'],'service UIDs');require(all(type(uid)is int and 0<uid<2**32-1 for uid in uids),'Non-root numeric service UIDs required');unique(uids,'Service UIDs')
    devices=array(request['devices'],'devices',True);unique(devices,'Devices')
    require(all(p in ['/dev/null','/dev/random','/dev/urandom'] for p in devices),'Unsupported device')
    net=request['network'];net_keys=['mode','namespace_path','sockets']
    if type(net) is dict and 'launch_channel' in net:
        net_keys.append('launch_channel')
        require(type(net['launch_channel']) is str and net['launch_channel']==LAUNCH_CHANNEL,
                'Unsupported process launch channel')
    keys(net,net_keys,'network')
    require(net['mode'] in ('private','shared-private'),'Only private staging networks are supported')
    if net['mode']=='private':require(net['namespace_path'] is None,'Private mode must not name a namespace')
    else:path(net['namespace_path'])
    sockets=[]
    for item in array(net['sockets'],'sockets',True):
        keys(item,['family','type'],'socket');require(item['family'] in ('unix','inet','inet6') and item['type'] in ('stream','dgram'),'Unsupported socket policy')
        sockets.append((item['family'],item['type']))
    unique(sockets,'Socket policy')
    return catalog,members,paths,sorted(writable),sorted(readonly),sorted(uids),sorted(devices),sorted(sockets)

def render(catalog_bytes, mapping, request):
    catalog,members,paths,writable,readonly,uids,devices,sockets=validate(catalog_bytes,mapping,request)
    binding={'catalog_sha512':request['catalog_sha512'],'mapping':mapping,'request':request}
    # Canonicalize semantically unordered local lists before deriving ownership identity.
    binding['mapping']={'schema':1,'members':sorted(mapping['members'],key=lambda x:x['id'])}
    normalized=dict(request)
    for key in ['service_uids','writable_roots','readonly_files','devices']:normalized[key]=sorted(request[key])
    if 'readonly_directories' in request:normalized['readonly_directories']=sorted(request['readonly_directories'])
    normalized['code_aliases']=sorted(request['code_aliases'],key=lambda x:(x['member_id'],x['path']))
    normalized['network']=dict(request['network']);normalized['network']['sockets']=sorted(request['network']['sockets'],key=lambda x:(x['family'],x['type']))
    binding['request']=normalized
    policy_id=hashlib.sha256(canonical(binding)).hexdigest();name='dyt-native-staging-'+policy_id[:24]
    launch_channel='launch_channel' in request['network']
    socket_families={'AF_'+f.upper() for f,t in sockets}
    if launch_channel:socket_families.add('AF_UNIX')
    code=sorted(p for ps in paths.values() for p in ps)
    # ReadWritePaths creates explicit nested mounts. Mark each one noexec rather
    # than relying on the parent mount's flag surviving mount construction.
    properties={'NoExecPaths':['/']+writable,'ExecPaths':code,'BindReadOnlyPaths':code,'ProtectSystem':'strict',
                'ReadOnlyPaths':sorted(readonly+request.get('readonly_directories',[])),'ReadWritePaths':writable,'NoNewPrivileges':True,
                'CapabilityBoundingSet':[],'RestrictNamespaces':True,'SystemCallArchitectures':['native'],
                'MemoryDenyWriteExecute':True,'SystemCallFilter':['~@mount','memfd_create','ptrace','process_vm_writev'],
                'SystemCallErrorNumber':'EPERM',
                'InaccessiblePaths':['/dev/shm'],'AppArmorProfile':name,'PrivateDevices':True,
                'PrivateTmp':True,'RestrictSUIDSGID':True,'RestrictAddressFamilies':sorted(socket_families)}
    # SystemCallFilter is one space-joined deny expression; do not emit separate allow lines.
    properties['SystemCallFilter']='~@mount memfd_create ptrace process_vm_writev recvmsg recvmmsg pidfd_getfd io_uring_setup io_uring_enter io_uring_register'
    if not sockets and not launch_channel:
        properties.pop('RestrictAddressFamilies')
        properties['SystemCallFilter'] += ' socket socketpair'
    if request['network']['mode']=='private':properties['PrivateNetwork']=True
    else:properties.update({'PrivateNetwork':False,'NetworkNamespacePath':request['network']['namespace_path']})
    lines=['abi <abi/4.0>,',
           '# STAGING ONLY. Rendering does not load or qualify this profile.',
           '# No external policy abstractions or profile fallback rules.',f'profile {name} {{']
    for mid in sorted(paths):
        permissions='rmix' if members[mid]['kind']=='executable' else 'rm'
        for p in sorted(paths[mid]):lines.append(f'  {p} {permissions},')
    for p in readonly:lines.append(f'  {p} r,')
    for p in sorted(request.get('readonly_directories',[])):lines.append(f'  {p}/ r,')
    for p in writable:lines.extend([f'  {p}/ r,',f'  {p}/** rwk,'])
    for p in devices:lines.append(f"  {p} {'rw' if p=='/dev/null' else 'r'},")
    lines += ['  owner /proc/[0-9]*/{stat,maps,status,comm,exe} r,',
              '  owner /proc/[0-9]*/attr/current r,',
              '  /proc/[0-9]*/net/{tcp,tcp6} r,',
              '  owner /proc/[0-9]*/{fd,map_files}/ r,',
              '  owner /proc/[0-9]*/{fd,map_files}/* r,',
              '  owner /proc/[0-9]*/task/ r,',
              '  owner /proc/[0-9]*/task/[0-9]*/{stat,status,comm} r,',
              '  deny /proc/**/mem w,',
              '  deny ptrace (trace, tracedby),',
              f'  ptrace (read, readby) peer={name},',
              f'  signal (send) peer={name},',
              '  signal (receive),']
    lines += [f'  network {family} {kind},' for family,kind in sockets]
    if launch_channel:
        # Creation has no address/peer constraint in the selected AppArmor parser.
        # The status channel grants no bind, listen, connect or ancillary import.
        lines += ['  unix (create) type=seqpacket,',
                  f'  unix (send, receive) type=seqpacket addr=none peer=(addr=none,label={name}),']
    lines += ['  # No change_profile, mount, capability or undeclared executable-mapping grants.',
              '  # Message-based FD import and io_uring are denied by the paired syscall policy.', '}','']
    files={}
    def put(file,obj):files[file]=(json.dumps(obj,sort_keys=True,indent=2)+'\n').encode()
    put('unit-properties.json',{'schema':1,'scope':'STAGING_RENDER_ONLY','profile_name':name,
        'policy_id':policy_id,'service_uids':uids,'properties':properties,
        'merge_rule':'The wrapper may add resource/lifecycle settings and one declared User UID. It must not weaken or replace these controls. Verify effective values before readiness.'})
    files['apparmor.profile']='\n'.join(lines).encode()
    put('validation.json',{'schema':1,'status':'RENDERED_NOT_ACTIVATED','catalog_sha512':request['catalog_sha512'],
        'mapping_sha256':hashlib.sha256(canonical(binding['mapping'])).hexdigest(),'policy_id':policy_id,'profile_name':name,
        'members':[dict(members[mid],paths=sorted(paths[mid])) for mid in sorted(members)],
        'host_files_verified':False,'authority_verified':False,'kernel_policy_qualified':False,
        'continuous_enforcement':False,'production_qualified':False,'g35_accepted':False})
    put('live-verification-requirements.json',{'schema':1,'mandatory':LIVE_REQUIREMENTS+(LAUNCH_CHANNEL_REQUIREMENTS if launch_channel else []),'verified':False,
        'scope':'Every item requires separate live evidence; local rendering is not authority.'})
    put('normalized-request.json',normalized)
    put('FILE_HASHES.json',{n:{'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)} for n,data in sorted(files.items())})
    return files

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for arg in ['catalog','mapping','request','output']:parser.add_argument('--'+arg,type=Path,required=True)
    args=parser.parse_args()
    try:
        files=render(args.catalog.read_bytes(),decode(args.mapping.read_bytes()),decode(args.request.read_bytes()))
        args.output.mkdir(parents=False,exist_ok=False)
        for name,data in files.items():
            with (args.output/name).open('xb') as output:output.write(data)
    except (Invalid,OSError,TypeError,KeyError) as error:
        parser.exit(2,'Policy rendering failed: '+str(error)+'\n')
    print(json.dumps({'status':'RENDERED_NOT_ACTIVATED','output':str(args.output),'files':len(files)}))

if __name__=='__main__':main()
