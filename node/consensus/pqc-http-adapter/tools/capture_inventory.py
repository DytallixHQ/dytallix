#!/usr/bin/env python3
"""Capture local source/package/artifact evidence. This is not a G35 verifier."""
import argparse,hashlib,json,pathlib,re,subprocess,tarfile,tomllib
ROOT=pathlib.Path(__file__).resolve().parents[1]
def digest(path):
 h=hashlib.sha256()
 with path.open('rb') as f:
  for chunk in iter(lambda:f.read(1024*1024),b''):h.update(chunk)
 return h.hexdigest()
def cmd(args):return subprocess.run(args,check=True,capture_output=True,text=True,timeout=30).stdout

def main():
 p=argparse.ArgumentParser();p.add_argument('--evidence',type=pathlib.Path,required=True);p.add_argument('--binary',type=pathlib.Path,required=True);a=p.parse_args();out=a.evidence.resolve();binary=a.binary.resolve()
 if (out/'INVENTORY.json').exists():raise SystemExit('Refuse to overwrite inventory')
 metadata=json.loads((out/'CARGO_METADATA.json').read_text());lock=tomllib.loads((ROOT/'Cargo.lock').read_text())
 locked={(x['name'],x['version']):x for x in lock['package']}
 artifacts=[]
 for line in (out/'BUILD_MESSAGES.jsonl').read_text().splitlines():
  event=json.loads(line)
  if event.get('reason')=='compiler-artifact':artifacts.append(event)
 compiled={x['package_id']for x in artifacts};modules=[];files={};archives=[]
 for package in metadata['packages']:
  if package['id']not in compiled or package.get('source')is None:continue
  folder=pathlib.Path(package['manifest_path']).parent
  # Registry archive and extracted source share their registry identifier.
  archive=folder.parents[2]/'cache'/folder.parent.name/(package['name']+'-'+package['version']+'.crate')
  expected=locked[(package['name'],package['version'])]['checksum']
  if digest(archive)!=expected:raise RuntimeError('Locked archive checksum mismatch: '+package['name'])
  count=0
  with tarfile.open(archive,'r:gz')as tar:
   for member in tar:
    parts=pathlib.PurePosixPath(member.name).parts
    if len(parts)<2:continue
    if parts[0]!=folder.name or '..'in parts or member.issym()or member.islnk():raise RuntimeError('Unexpected archive path')
    if not member.isfile():continue
    source=folder.joinpath(*parts[1:]);recorded=digest(source)
    entry=tar.extractfile(member);h=hashlib.sha256()
    for block in iter(lambda:entry.read(1024*1024),b''):h.update(block)
    if h.hexdigest()!=recorded:raise RuntimeError('Extracted source differs from locked archive: '+str(source))
    files[str(source)]=recorded;count+=1
  modules.append({'name':package['name'],'version':package['version'],'license':package.get('license'),'archive_sha256':expected,'source_directory':str(folder),'files_verified':count,'build_target_kinds':sorted({kind for x in artifacts if x['package_id']==package['id']for kind in x['target']['kind']})})
  archives.append({'path':str(archive),'sha256':expected})
 source_before=json.loads((out/'SOURCE_BEFORE_BUILD.json').read_text())
 for path,expected in source_before.items():
  if digest(pathlib.Path(path))!=expected:raise RuntimeError('Build source changed')
 parent=json.loads((out/'PARENT_WORKSPACE_BEFORE.json').read_text())
 parent_unchanged=all(digest(pathlib.Path(path))==expected for path,expected in parent.items())
 rustc=cmd(['rustc','-vV']);compiler=pathlib.Path(cmd(['rustup','which','rustc']).strip());cargo=pathlib.Path(cmd(['rustup','which','cargo']).strip())
 symbols=cmd(['xcrun','nm','-a',str(binary)]);links=cmd(['xcrun','otool','-L',str(binary)])
 (out/'ARTIFACT_SYMBOLS.txt').write_text(symbols);(out/'ARTIFACT_LINKS.txt').write_text(links);(out/'RUSTC.txt').write_text(rustc)
 # Only a limited diagnostic. Thread-local storage symbols containing "tls"
 # are not classified as Transport Layer Security implementations.
 patterns=[r'rustls',r'openssl',r'boringssl',r'\bed25519',r'\bcurve25519',r'\bsecp256',r'\becdsa',r'\becdh',r'\bRSA_(?:new|public|private)',r'ring::signature']
 matches=[line for line in symbols.splitlines()if any(re.search(pattern,line,re.I)for pattern in patterns)]
 node_by_id={x['id']:x for x in metadata['resolve']['nodes']}
 feature_graph={x['name']+'@'+x['version']:node_by_id[x['id']]['features']for x in metadata['packages']if x['id']in compiled}
 inventory={'schema_version':1,'scope':'native diagnostic adapter build','binary':{'path':str(binary),'sha256':digest(binary),'size_bytes':binary.stat().st_size,'architecture':'aarch64-apple-darwin'},'build_source_hashes':source_before,'parent_workspace_unchanged':parent_unchanged,'compiled_packages_including_local':len(compiled),'verified_registry_packages':modules,'verified_registry_source_file_count':len(files),'compiler':{'path':str(compiler),'sha256':digest(compiler),'version_record':'RUSTC.txt'},'cargo':{'path':str(cargo),'sha256':digest(cargo)},'feature_graph':feature_graph,'diagnostic_symbol_search':{'patterns':patterns,'matches':matches,'proves_classical_exclusion':False},'dynamic_links_record':'ARTIFACT_LINKS.txt','limits':['Build package list includes build scripts and procedural macros; it is not a runtime-only SBOM.','Source verification covers complete used registry archives, including files not compiled for this target.','Symbols are a limited known-name diagnostic. Renamed, unknown or provider implementations are not excluded by this check.','The Rust standard-library rlibs, native linker inputs, dynamic libraries and transitive operating-system providers need separate compiled-provider qualification.','No Linux runtime or hosted HTTPS ingress is qualified by this native adapter capture.'],'production_acceptance_granted':False}
 (out/'REGISTRY_SOURCE_DIGESTS.json').write_text(json.dumps(files,indent=2)+'\n');(out/'REGISTRY_ARCHIVES.json').write_text(json.dumps(archives,indent=2)+'\n');(out/'INVENTORY.json').write_text(json.dumps(inventory,indent=2)+'\n')
 print(json.dumps({'registry_packages':len(modules),'source_files_verified':len(files),'symbol_pattern_matches':len(matches),'parent_workspace_unchanged':parent_unchanged}))
if __name__=='__main__':main()
