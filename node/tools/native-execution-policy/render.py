#!/usr/bin/env python3
"""Render staging and production service policy. This program never verifies or activates host policy."""
import argparse
import hashlib
import ipaddress
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
STAGING_PROFILES = ('development-linux-native-service-v2', 'development-linux-native-service-http-v2')
# One production catalog serves every node role and carries the adapter
# (production activation v1, A5). It runs only in the routed network mode.
PRODUCTION_PROFILE = 'production-linux-native-service-v1'
PRODUCTION_TRANSPORT = 'dytallix-pqc-production-v1'
TRANSPORT_KEYS = ['version', 'profile', 'network', 'local_public_key_base64', 'peers', 'handshake_timeout_ms']
MAX_PINS = 64
MAX_PUBLIC_LISTENERS = 4
ROUTED_REQUIREMENTS = [
    'Load host-firewall.nft before the unit starts. Verify the effective input policy drops every source except the pinned peers at the P2P listener, the declared public listeners, established replies, loopback and the listed ICMP types.',
    'Verify the node listens on and sends from its single address; a public sentry uses static 1:1 NAT without port translation (P01, 30 September 2026).',
    'Render and load the rules again with each reviewed pin change, before the restart that applies it.',
]
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
    'Verify the effective MemoryMax, MemorySwapMax, TasksMax, LimitNOFILE and LimitCORE of the unit and every child equal the rendered values before readiness.',
    'Verify one declared numeric non-root service UID per unit, external harness separation, and declared inherited descriptors only.',
    'For shared-private networking verify the named namespace belongs to the owned anchor and differs from the host namespace.',
    'Verify recvmsg/recvmmsg and io_uring denial is compatible with exact Go/Rust socket paths; never silently relax descriptor-import controls.',
    'Verify root helper executes the immutable catalog path with the qualified handshake; no writable executable snapshot exception.',
    'Qualify inert positive/denial probes and exact workload startup, commitment, receipts, helper behavior, shutdown and restart.',
    'Keep sampled mapping observation, policy prevention, provider source assurance and production approval as separate records.',
]

class Invalid(ValueError):
    pass

# The unit's syscall policy: one systemd deny expression. Message-based FD
# import and io_uring are denied with the AppArmor rules below.
SYSTEM_CALL_DENY = ('~@mount memfd_create ptrace process_vm_writev recvmsg recvmmsg '
                    'pidfd_getfd io_uring_setup io_uring_enter io_uring_register')
# Added when the unit has no network sockets.
NO_SOCKET_DENY = 'socket socketpair'
# Unit resource limits (E04 gap 13, P01 28 September 2026): required, with no
# defaults; the values are the operator's (D06-Q02, D12-Q01). The bounds are
# kernel limits, not policy: 1 PiB, PID_MAX_LIMIT and the default fs.nr_open.
RESOURCE_BOUNDS = {'memory_max_bytes': 2**50, 'tasks_max': 4194304, 'nofile': 1048576}


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

def endpoint(value):
    """A canonical IP:port the production engine accepts: global unicast
    (private ranges included, as Go defines it) and a port from 1024."""
    require(type(value) is str and re.fullmatch(r'[0-9a-f.:\[\]]{1,64}:[0-9]{1,5}', value), 'Invalid endpoint')
    host, port = value.rsplit(':', 1)
    if host.startswith('['):
        require(host.endswith(']'), 'Invalid endpoint')
        host = host[1:-1]
    try:
        ip = ipaddress.ip_address(host)
    except ValueError as error:
        raise Invalid('Invalid endpoint') from error
    text = f'[{ip.compressed}]:{int(port)}' if ip.version == 6 else f'{ip.compressed}:{int(port)}'
    require(text == value and 1024 <= int(port) <= 65535, 'Noncanonical endpoint')
    require(not (ip.is_loopback or ip.is_unspecified or ip.is_multicast or ip.is_link_local or
                 (ip.version == 4 and ip == ipaddress.IPv4Address('255.255.255.255'))),
            'Endpoint must be a global unicast address')
    return ip, int(port)

def firewall_sources(spec, transport, chain_id):
    """The pinned peer addresses from the node's exact transport file, the
    P2P listener and the public listeners (production activation v1, A5)."""
    keys(spec, ['transport_sha256', 'p2p_listen', 'public_listeners'], 'firewall')
    require(type(transport) is bytes and hashlib.sha256(transport).hexdigest() == digest(spec['transport_sha256'], 64),
            'Transport file digest mismatch')
    tc = decode(transport)
    keys(tc, TRANSPORT_KEYS, 'transport')
    require(json.dumps(tc, separators=(',', ':')).encode() == transport.strip(), 'Transport must be canonical compact JSON')
    require(tc['version'] == 1 and tc['profile'] == PRODUCTION_TRANSPORT and tc['network'] == chain_id,
            'Production transport for this chain required')
    listen = endpoint(spec['p2p_listen'])
    public = [endpoint(p) for p in array(spec['public_listeners'], 'public listeners', True)]
    require(len(public) <= MAX_PUBLIC_LISTENERS, 'Public listener bound')
    unique([p[1] for p in public] + [listen[1]], 'Listener ports')
    require(all(ip == listen[0] for ip, _ in public), 'Public listeners use the node address')
    peers = []
    for peer in array(tc['peers'], 'peers'):
        keys(peer, ['id', 'public_key_base64', 'address'], 'peer')
        peers.append(endpoint(peer['address'])[0])
    require(len(peers) <= MAX_PINS, 'At most 64 pinned peers')
    unique(peers, 'Pinned peer addresses')
    require(listen[0] not in peers, 'A pinned peer uses the node address')
    require(all(ip.version == listen[0].version for ip in peers), 'Pinned peers use the node address family')
    return listen, public, sorted(peers)

def firewall_rules(listen, public, peers):
    """Inbound rules for nft: only pinned peers reach the P2P listener, so an
    unpinned source is dropped before the handshake. Outbound is unchanged."""
    family = 'ip' if listen[0].version == 4 else 'ip6'
    lines = ['#!/usr/sbin/nft -f',
             '# Dytallix host firewall (production activation v1). Rendering does not load it.',
             'table inet dytallix_node', 'delete table inet dytallix_node',
             'table inet dytallix_node {', '  chain input {',
             '    type filter hook input priority filter; policy drop;',
             '    ct state invalid drop', '    ct state established,related accept', '    iif "lo" accept',
             '    icmp type { destination-unreachable, time-exceeded, parameter-problem } accept',
             '    icmpv6 type { destination-unreachable, packet-too-big, time-exceeded, parameter-problem, '
             'nd-router-advert, nd-neighbor-solicit, nd-neighbor-advert, mld-listener-query } accept',
             f'    {family} saddr {{ {", ".join(ip.compressed for ip in peers)} }} {family} daddr '
             f'{listen[0].compressed} tcp dport {listen[1]} accept']
    lines += [f'    {family} daddr {ip.compressed} tcp dport {port} accept' for ip, port in sorted(public, key=lambda x: x[1])]
    return '\n'.join(lines + ['  }', '}', '']).encode()

def validate(catalog_bytes, mapping, request, transport=None):
    catalog = decode(catalog_bytes)
    request_keys = ['schema','policy_version','catalog_sha512','service_uids','writable_roots',
                    'readonly_files','code_aliases','devices','network','resources']
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
    require(catalog['service_profile'] in STAGING_PROFILES + (PRODUCTION_PROFILE,),
            'Only native staging and production service profiles are supported')
    production = catalog['service_profile'] == PRODUCTION_PROFILE
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
    required=ROLES | ({'http_adapter'} if '-http-' in catalog['service_profile'] or production else set())
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
    if type(net) is dict and net.get('mode')=='routed':net_keys.append('firewall')
    keys(net,net_keys,'network')
    require(net['mode'] in ('private','shared-private','routed'),'Unsupported network mode')
    # A production node routes on the host network behind its rendered
    # firewall; staging catalogs stay in private namespaces.
    require((net['mode']=='routed')==production,'Only the production catalog runs routed, and it runs only routed')
    sources=None
    if net['mode']=='private':require(net['namespace_path'] is None,'Private mode must not name a namespace')
    elif net['mode']=='routed':
        require(net['namespace_path'] is None,'Routed mode must not name a namespace')
        sources=firewall_sources(net['firewall'],transport,catalog['chain_id'])
    else:path(net['namespace_path'])
    sockets=[]
    for item in array(net['sockets'],'sockets',True):
        keys(item,['family','type'],'socket');require(item['family'] in ('unix','inet','inet6') and item['type'] in ('stream','dgram'),'Unsupported socket policy')
        sockets.append((item['family'],item['type']))
    unique(sockets,'Socket policy')
    if sources:
        require(('inet' if sources[0][0].version==4 else 'inet6','stream') in sockets,
                'Routed mode needs a stream socket of the node address family')
    resources=request['resources'];keys(resources,sorted(RESOURCE_BOUNDS),'resources')
    for name,bound in RESOURCE_BOUNDS.items():
        require(type(resources[name]) is int and 0<resources[name]<=bound,'Explicit positive bounded resource limits required')
    return catalog,members,paths,sorted(writable),sorted(readonly),sorted(uids),sorted(devices),sorted(sockets),resources,sources

def render(catalog_bytes, mapping, request, transport=None):
    catalog,members,paths,writable,readonly,uids,devices,sockets,resources,sources=validate(catalog_bytes,mapping,request,transport)
    binding={'catalog_sha512':request['catalog_sha512'],'mapping':mapping,'request':request}
    # Canonicalize semantically unordered local lists before deriving ownership identity.
    binding['mapping']={'schema':1,'members':sorted(mapping['members'],key=lambda x:x['id'])}
    normalized=dict(request)
    for key in ['service_uids','writable_roots','readonly_files','devices']:normalized[key]=sorted(request[key])
    if 'readonly_directories' in request:normalized['readonly_directories']=sorted(request['readonly_directories'])
    normalized['code_aliases']=sorted(request['code_aliases'],key=lambda x:(x['member_id'],x['path']))
    normalized['network']=dict(request['network']);normalized['network']['sockets']=sorted(request['network']['sockets'],key=lambda x:(x['family'],x['type']))
    if sources:
        normalized['network']['firewall']=dict(request['network']['firewall'],public_listeners=sorted(request['network']['firewall']['public_listeners']))
    # The profile identity binds what the AppArmor profile enforces. Resource
    # limits are unit properties and the firewall is a host rule set:
    # FILE_HASHES.json and the four-role identity bind them, and a change to
    # either does not rename this profile.
    binding['request']={k:v for k,v in normalized.items() if k!='resources'}
    if sources:binding['request']['network']={k:v for k,v in normalized['network'].items() if k!='firewall'}
    policy_id=hashlib.sha256(canonical(binding)).hexdigest()
    name=('dyt-native-production-' if sources else 'dyt-native-staging-')+policy_id[:24]
    launch_channel='launch_channel' in request['network']
    socket_families={'AF_'+f.upper() for f,t in sockets}
    if launch_channel:socket_families.add('AF_UNIX')
    code=sorted(p for ps in paths.values() for p in ps)
    # ReadWritePaths creates explicit nested mounts. Mark each one noexec rather
    # than relying on the parent mount's flag surviving mount construction.
    properties={'NoExecPaths':['/']+writable,'ExecPaths':code,'BindReadOnlyPaths':code,'ProtectSystem':'strict',
                'ReadOnlyPaths':sorted(readonly+request.get('readonly_directories',[])),'ReadWritePaths':writable,'NoNewPrivileges':True,
                'CapabilityBoundingSet':[],'RestrictNamespaces':True,'SystemCallArchitectures':['native'],
                'MemoryDenyWriteExecute':True,
                'SystemCallErrorNumber':'EPERM',
                'InaccessiblePaths':['/dev/shm'],'AppArmorProfile':name,'PrivateDevices':True,
                'PrivateTmp':True,'RestrictSUIDSGID':True,'RestrictAddressFamilies':sorted(socket_families),
                # One cgroup holds every node process; LimitNOFILE applies to each.
                'MemoryMax':resources['memory_max_bytes'],'TasksMax':resources['tasks_max'],
                'LimitNOFILE':resources['nofile'],
                # Keys and signing state never reach swap or a core file.
                'MemorySwapMax':0,'LimitCORE':0}
    # SystemCallFilter is one space-joined deny expression; do not emit separate allow lines.
    properties['SystemCallFilter']=SYSTEM_CALL_DENY
    if not sockets and not launch_channel:
        properties.pop('RestrictAddressFamilies')
        properties['SystemCallFilter'] += ' ' + NO_SOCKET_DENY
    if request['network']['mode']=='private':properties['PrivateNetwork']=True
    elif sources:properties['PrivateNetwork']=False
    else:properties.update({'PrivateNetwork':False,'NetworkNamespacePath':request['network']['namespace_path']})
    lines=['abi <abi/4.0>,',
           '# PRODUCTION CATALOG. Rendering does not load or qualify this profile.' if sources else
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
        'merge_rule':'The wrapper may add lifecycle settings and one declared User UID. It must not weaken or replace these controls, including the resource limits. Verify effective values before readiness.'})
    files['apparmor.profile']='\n'.join(lines).encode()
    put('validation.json',{'schema':1,'status':'RENDERED_NOT_ACTIVATED','catalog_sha512':request['catalog_sha512'],
        'mapping_sha256':hashlib.sha256(canonical(binding['mapping'])).hexdigest(),'policy_id':policy_id,'profile_name':name,
        'members':[dict(members[mid],paths=sorted(paths[mid])) for mid in sorted(members)],
        'host_files_verified':False,'authority_verified':False,'kernel_policy_qualified':False,
        'continuous_enforcement':False,'production_qualified':False,'g35_accepted':False})
    put('live-verification-requirements.json',{'schema':1,'mandatory':LIVE_REQUIREMENTS+(LAUNCH_CHANNEL_REQUIREMENTS if launch_channel else [])+(ROUTED_REQUIREMENTS if sources else []),'verified':False,
        'scope':'Every item requires separate live evidence; local rendering is not authority.'})
    if sources:files['host-firewall.nft']=firewall_rules(*sources)
    put('normalized-request.json',normalized)
    put('FILE_HASHES.json',{n:{'sha256':hashlib.sha256(data).hexdigest(),'bytes':len(data)} for n,data in sorted(files.items())})
    return files

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for arg in ['catalog','mapping','request','output']:parser.add_argument('--'+arg,type=Path,required=True)
    # The node's exact pqc_transport.json; required in routed mode.
    parser.add_argument('--transport',type=Path)
    args=parser.parse_args()
    try:
        files=render(args.catalog.read_bytes(),decode(args.mapping.read_bytes()),decode(args.request.read_bytes()),
                     args.transport.read_bytes() if args.transport else None)
        args.output.mkdir(parents=False,exist_ok=False)
        for name,data in files.items():
            with (args.output/name).open('xb') as output:output.write(data)
    except (Invalid,OSError,TypeError,KeyError) as error:
        parser.exit(2,'Policy rendering failed: '+str(error)+'\n')
    print(json.dumps({'status':'RENDERED_NOT_ACTIVATED','output':str(args.output),'files':len(files)}))

if __name__=='__main__':main()
