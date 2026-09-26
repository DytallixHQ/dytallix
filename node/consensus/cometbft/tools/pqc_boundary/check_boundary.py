#!/usr/bin/env python3
"""Inventory one Go executable. This tool never grants chain-wide G35 approval."""
import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

PROFILE = 'pqc-engine-v1'
PACKAGE = './cmd/dytallix-pqc-engine'
IMPORT = 'dytallix.local/consensus/cometbft/cmd/dytallix-pqc-engine'
TAG = 'dytallix_pqc_only'
IPC_TAG = 'dytallix_pqc_only,dytallix_pqc_ipc'
ALLOWED_TAGS = (TAG, IPC_TAG)
# Symmetric encryption and hashes are not classical asymmetric cryptography.
# Package rules deliberately include TLS/X.509 containers, even if the linker
# removes an individual method. This is a source-graph exclusion policy.
RULES = {
 'standard_classical': r'^(crypto/(rsa|dsa|ecdsa|ecdh|ed25519|elliptic|tls|x509)(/|$)|crypto/internal/(fips140/)?(rsa|ecdsa|ecdh|ed25519|edwards25519|nistec|bigmod)(/|$))',
 'extended_classical': r'^(golang.org/x/crypto/(curve25519|ed25519|edwards25519|bn256|openpgp|otr|ssh)(/|$)|filippo.io/edwards25519(/|$)|github.com/(oasisprotocol/curve25519-voi|decred/dcrd/dcrec|btcsuite/btcd/btcec|davidlazar/go-crypto)(/|$))',
 'legacy_comet_keys': r'^github.com/cometbft/cometbft/crypto/(ed25519|secp256k1|secp256k1eth|sr25519|bls12381)(/|$)',
 'legacy_transport': r'^(github.com/(libp2p/go-libp2p|flynn/noise|quic-go/|pion/(dtls|webrtc))|github.com/cometbft/cometbft/lp2p(/|$))',
 'circl_classical': r'^github.com/cloudflare/circl/(ecc|dh|sign/(ed25519|ed448)|kem/(sike|sidh|x25519|x448))(/|$)',
}
PROVIDER_RULES = {'provider_container_requires_review': r'^crypto/internal/boring(/|$)'}
SYMBOL_EXTRA = re.compile(r'(SecretConnection|MakeSecretConnection|secp256k1|curve25519|edwards25519|crypto/tls[./]|crypto/rsa[./]|crypto/ecdsa[./]|crypto/ecdh[./]|crypto/ed25519[./])')
FILE_FIELDS = ('GoFiles','CgoFiles','CFiles','CXXFiles','MFiles','HFiles','FFiles','SFiles','SwigFiles','SwigCXXFiles','SysoFiles','EmbedFiles')
PROVIDER_EXCEPTION_SHA256 = '90b7bfd54b4a5d64afca3679e4996d579c09dd685d8b7c14acc5e130adbf83a0'


def digest_bytes(data):
 return hashlib.sha256(data).hexdigest()


def digest(path):
 h=hashlib.sha256()
 with open(path,'rb') as stream:
  for block in iter(lambda:stream.read(1024*1024),b''):h.update(block)
 return h.hexdigest()


def canonical_hash(value):
 return digest_bytes(json.dumps(value,sort_keys=True,separators=(',',':')).encode())


def json_stream(value):
 decoder=json.JSONDecoder(); items=[]
 while value.strip():
  value=value.lstrip(); item,end=decoder.raw_decode(value);items.append(item);value=value[end:]
 if not items:raise ValueError('empty dependency graph')
 return items


def classify_package(name):
 return sorted(key for key,pattern in RULES.items() if re.search(pattern,name))


def classify_provider_package(name):
 return sorted(key for key,pattern in PROVIDER_RULES.items() if re.search(pattern,name))


def classify_provider_symbol(name):
 reasons=set()
 for end in [len(name)]+[m.start() for m in re.finditer(r'[.()]',name)]:
  reasons.update(classify_provider_package(name[:end]))
 return sorted(reasons)


def classify_symbol(name):
 # Go symbols append method/type suffixes with a dot. Import paths can also
 # contain dots, so test prefixes at each possible boundary, not the first dot.
 reasons=set()
 for end in [len(name)]+[m.start() for m in re.finditer(r'[.()]',name)]:
  reasons.update(classify_package(name[:end]))
 if SYMBOL_EXTRA.search(name):reasons.add('classical_or_secret_connection_symbol')
 if re.match(r'^_?(SecTrust|SecCertificate|SecPolicy|SSL_|EVP_|RSA_|ECDSA_|ECDH_|X509_)',name):reasons.add('native_crypto_or_trust_symbol')
 return sorted(reasons)


def parse_symbols(raw):
 if not raw.strip() or 'no symbol' in raw.lower():raise ValueError('symbols absent or stripped')
 result=[]
 for line in raw.splitlines():
  parts=line.strip().split(maxsplit=2)
  if len(parts)==3 and re.fullmatch(r'[0-9a-fA-F]+',parts[0]) and len(parts[1])==1:
   result.append(parts[2])
  elif len(parts)==2 and parts[0]=='U':
   result.append(parts[1])
  elif line.strip():raise ValueError('unrecognized symbol output')
 if len(result)<2 or 'runtime.main' not in result:raise ValueError('incomplete symbol table')
 return result


def shortest_path(graph,root,target):
 queue=collections.deque([[root]]);seen={root}
 while queue:
  path=queue.popleft()
  if path[-1]==target:return path
  for child in graph.get(path[-1],[]):
   if child not in seen:seen.add(child);queue.append(path+[child])
 return []


def run(args,cwd,env):
 result=subprocess.run(args,cwd=cwd,env=env,text=True,capture_output=True,timeout=600)
 if result.returncode:raise ValueError(f'{args[1]} failed: {result.stderr.strip()[:2000]}')
 return result.stdout


def source_manifest(packages):
 files={}; locks={}
 for package in packages:
  directory=Path(package['Dir'])
  for field in FILE_FIELDS:
   for name in package.get(field,[]):
    path=directory/name
    files[str(path.resolve())]=digest(path)
  module=package.get('Module',{})
  actual=module.get('Replace',module)
  if actual.get('Dir'):
   for filename in ('go.mod','go.sum'):
    path=Path(actual['Dir'])/filename
    if path.is_file():locks[str(path.resolve())]=digest(path)
 return {'files':dict(sorted(files.items())), 'locks':dict(sorted(locks.items()))}


def check_binary(path,expected):
 if not path.is_file():raise ValueError('binary is missing or not a regular file')
 actual=digest(path)
 if expected and actual!=expected:raise ValueError('binary SHA256 mismatch')
 return actual


def load_provider_exception(path):
 # Compile only the bytes whose digest was checked. Python's source loader can
 # otherwise execute a timestamp-valid __pycache__ file instead of the source.
 source=path.read_bytes()
 if digest_bytes(source)!=PROVIDER_EXCEPTION_SHA256:raise ValueError('provider exception source pin mismatch')
 namespace={'__name__':'pinned_provider_exception','__file__':str(path)}
 exec(compile(source,str(path),'exec',dont_inherit=True),namespace)
 review=namespace.get('review')
 if not callable(review):raise ValueError('provider exception review is missing')
 return review


def inspect(args):
 report={'schema_version':1,'profile':args.profile,'launch_status':'NO_GO','g35_status':'NOT_GRANTED',
  'scope':'Selected Go executable and selected source dependency graph only',
  'classification_notes':['Prohibited entries include algorithm implementations and excluded protocol containers; a match is not an algorithm count.',
   'Provider review entries can contain disabled backend stubs or marker functions. Their presence alone does not prove a classical algorithm implementation.'],
  'limitations':['Known-name rules cannot prove absence of renamed, novel, or obfuscated algorithms.',
   'This inventory does not qualify validators, transactions, other executables, custody, root authority, or production deployment.',
   'A byte-identical rebuild binds observed inputs to the executable; it is not independent review or trusted release provenance.'],
  'boundary_status':'FAIL','errors':[]}
 try:
  if args.profile!=PROFILE:raise ValueError('unknown profile')
  if args.tags not in ('',*ALLOWED_TAGS):raise ValueError('unknown build tag selection')
  report['checker']={'path':str(Path(__file__).resolve()),'sha256':digest(__file__)}
  binary=Path(args.binary).resolve();directory=Path(args.module_dir).resolve()
  binary_hash=check_binary(binary,args.expected_sha256)
  report['binary']={'path':str(binary),'sha256':binary_hash,'size':binary.stat().st_size}
  go=shutil.which('go')
  if not go:raise ValueError('Go toolchain not found')
  env=dict(os.environ,GOFLAGS='',GOWORK='off',GOPROXY='off',GOSUMDB='off',GOTOOLCHAIN='local')
  version=run([go,'version','-m',str(binary)],directory,env)
  report['go_version_m']=version
  build=dict(re.findall(r'^\s*build\s+([^=\n]+)=(.*)$',version,re.M))
  main=re.search(r'^\s*path\s+(\S+)',version,re.M)
  if not main or main.group(1)!=IMPORT:raise ValueError('unexpected executable package')
  toolenv=json.loads(run([go,'env','-json','GOROOT','GOTOOLDIR','GOVERSION','GOOS','GOARCH','CGO_ENABLED'],directory,env))
  toolfiles=[Path(go).resolve()]+[Path(run([go,'tool','-n',name],directory,env).strip()) for name in ('compile','link','asm','nm')]
  report['toolchain']={'environment':toolenv,'files':{str(p):digest(p) for p in toolfiles}}
  report['toolchain']['sha256']=canonical_hash(report['toolchain']['files'])
  if not version.splitlines()[0].endswith(toolenv['GOVERSION']):report['errors'].append('binary and inspecting toolchain versions differ')
  for key in ('CGO_ENABLED','GOOS','GOARCH','GOARM64','GOAMD64'):
   if key in build:env[key]=build[key]
  graph_cmd=[go,'list','-mod=readonly','-deps','-json']
  if args.tags:graph_cmd.append('-tags='+args.tags)
  graph_cmd.append(PACKAGE)
  packages=json_stream(run(graph_cmd,directory,env))
  if any(p.get('Error') or p.get('DepsErrors') or p.get('Incomplete') for p in packages):raise ValueError('incomplete dependency graph')
  graph={p['ImportPath']:p.get('Imports',[]) for p in packages}
  manifest=source_manifest(packages)
  if not any(k.endswith('/go.sum') for k in manifest['locks']):raise ValueError('module lock go.sum missing')
  report['source']={'manifest':manifest,'sha256':canonical_hash(manifest),'package_count':len(packages),
   'graph':graph,'selected_build_tags':args.tags,
   'ignored_files':{p['ImportPath']:p.get('IgnoredGoFiles',[]) for p in packages if p.get('IgnoredGoFiles')}}
  report['prohibited_packages']=[{'package':name,'rules':classify_package(name),'shortest_import_path':shortest_path(graph,IMPORT,name)} for name in sorted(graph) if classify_package(name)]
  report['provider_review_packages']=[{'package':name,'rules':classify_provider_package(name),'shortest_import_path':shortest_path(graph,IMPORT,name)} for name in sorted(graph) if classify_provider_package(name)]
  raw=run([go,'tool','nm',str(binary)],directory,env)
  symbols=parse_symbols(raw)
  report['symbols']={'count':len(symbols),'raw_sha256':digest_bytes(raw.encode()),'all':symbols}
  report['prohibited_symbols']=[{'symbol':name,'rules':classify_symbol(name)} for name in symbols if classify_symbol(name)]
  report['provider_review_symbols']=[{'symbol':name,'rules':classify_provider_symbol(name)} for name in symbols if classify_provider_symbol(name)]
  if report['prohibited_packages']:report['errors'].append('prohibited packages in selected source graph')
  if report['prohibited_symbols']:report['errors'].append('prohibited symbols in executable')
  if build.get('CGO_ENABLED')!='0':report['errors'].append('CGO or external native code not excluded')
  if args.tags not in ALLOWED_TAGS or build.get('-tags')!=args.tags:report['errors'].append('required build tag is not bound to binary')
  if build.get('-trimpath')!='true':report['errors'].append('required trimpath build setting absent')
  if any(k.startswith('vcs') for k in build):report['errors'].append('profile requires buildvcs=false')
  report['rebuild']={'requested':args.rebuild,'byte_identical':False}
  if args.rebuild:
   if build.get('-ldflags'):raise ValueError('profile rejects custom linker flags, including stripping')
   if build.get('-compiler','gc')!='gc' or build.get('-buildmode','exe')!='exe':raise ValueError('unsupported compiler or build mode')
   with tempfile.TemporaryDirectory(prefix='dyt-pqc-boundary-') as temp:
    output=Path(temp)/'engine'
    command=[go,'build','-mod=readonly','-trimpath','-buildvcs=false','-tags='+args.tags,'-p=1','-o',str(output),PACKAGE]
    report['rebuild']['command']=command[:-2]+['<temporary-executable>',PACKAGE]
    run(command,directory,env)
    rebuilt_hash=digest(output);report['rebuild']['sha256']=rebuilt_hash
    report['rebuild']['byte_identical']=rebuilt_hash==binary_hash
  if not report['rebuild']['byte_identical']:report['errors'].append('source-to-executable binding not proven by byte-identical rebuild')
  # Verify inputs again. Do not silently bind a pre-edit graph to a post-edit build.
  after=json_stream(run(graph_cmd,directory,env))
  if canonical_hash(source_manifest(after))!=report['source']['sha256']:report['errors'].append('source or locks changed during inspection')
  if {p['ImportPath']:p.get('Imports',[]) for p in after}!=graph:report['errors'].append('dependency graph changed during inspection')
  if digest(binary)!=binary_hash:report['errors'].append('binary changed during inspection')
  if {str(p):digest(p) for p in toolfiles}!=report['toolchain']['files']:report['errors'].append('toolchain changed during inspection')
  if digest(__file__)!=report['checker']['sha256']:report['errors'].append('checker changed during inspection')
  if report['provider_review_packages'] or report['provider_review_symbols']:
   try:
    exception_path=Path(__file__).with_name('provider_exception.py')
    review=load_provider_exception(exception_path)
    report['provider_exception']=review(report,packages)
    report['provider_exception']['source_sha256']=PROVIDER_EXCEPTION_SHA256
    if digest(exception_path)!=PROVIDER_EXCEPTION_SHA256:raise ValueError('provider exception changed during inspection')
   except Exception as exc:
    report.pop('provider_exception',None)
    report['errors'].append('crypto provider containers require separate source and build review: '+str(exc))
  if not report['errors']:report['boundary_status']='KNOWN_CLASSICAL_EXCLUSIONS_CHECKED'
 except (OSError,ValueError,KeyError,subprocess.SubprocessError) as exc:
  report['errors'].append(str(exc))
 report['inventory_sha256']=canonical_hash(report)
 return report


def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--module-dir',required=True)
 parser.add_argument('--binary',required=True)
 parser.add_argument('--profile',required=True)
 parser.add_argument('--tags',default='')
 parser.add_argument('--expected-sha256')
 parser.add_argument('--rebuild',action='store_true')
 parser.add_argument('--output',required=True)
 args=parser.parse_args();report=inspect(args)
 Path(args.output).write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
 print(json.dumps({key:report.get(key) for key in ('boundary_status','launch_status','g35_status','errors','inventory_sha256')}))
 return 0 if report['boundary_status']=='KNOWN_CLASSICAL_EXCLUSIONS_CHECKED' else 1

if __name__=='__main__':sys.exit(main())
