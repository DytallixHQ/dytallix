#!/usr/bin/env python3
"""Build public browser codecs from locked source in fresh target directories.

This tool never writes the Site checkout, installs tools, or approves production.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time

SDK = Path(__file__).resolve().parents[3]
TOOLCHAIN = '1.88.0'
BINDGEN_VERSION = 'wasm-bindgen 0.2.100'
TARGET = 'wasm32-unknown-unknown'
ASSETS = ('ordinary_browser.js', 'ordinary_browser.d.ts', 'ordinary_browser_bg.wasm', 'ordinary_browser_bg.wasm.d.ts')
DISK_FLOOR = 2 * 1024**3


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2, sort_keys=True); stream.write('\n')


def source_manifest():
    paths = [SDK/'Cargo.toml', SDK/'Cargo.lock', SDK/'rust-toolchain.toml']
    for folder in ('crates/ordinary-browser', 'vendor/dytallix-protocol-types'):
        paths += [p for p in (SDK/folder).rglob('*') if p.is_file() and
                  not any(part in {'target', '__pycache__', '.git'} for part in p.relative_to(SDK).parts)]
    return {str(p.relative_to(SDK)): digest(p) for p in sorted(set(paths))}


def stop_process_group(process, grace_seconds=5):
    """Stop the owned build session before its temporary files can be removed."""
    group = process.pid
    try:
        os.killpg(group, signal.SIGTERM)
    except ProcessLookupError:
        process.wait(timeout=5)
        return
    deadline = time.monotonic() + grace_seconds
    while time.monotonic() < deadline:
        process.poll()  # Reap the direct child when it exits.
        try:
            os.killpg(group, 0)
        except ProcessLookupError:
            process.wait(timeout=5)
            return
        time.sleep(0.05)
    # The direct cargo process can exit before its compiler children. Kill the
    # session's group even when process.poll() already reports an exit status.
    try:
        os.killpg(group, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=5)


def run(command, env, output, timeout=300):
    with output.open('xb') as log:
        process = subprocess.Popen(command, cwd=SDK, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
        started = time.monotonic()
        try:
            while process.poll() is None:
                if time.monotonic()-started > timeout:
                    raise RuntimeError('Build command exceeded time bound')
                if shutil.disk_usage(output.parent).free < DISK_FLOOR:
                    raise RuntimeError('Build stopped at the 2 GiB disk floor')
                if output.stat().st_size > 4 * 1024**2:
                    raise RuntimeError('Build log exceeded 4 MiB')
                time.sleep(0.2)
            if process.returncode:
                raise RuntimeError('Build command failed; see '+str(output))
        except BaseException:
            stop_process_group(process)
            raise


def build(args):
    if shutil.disk_usage(args.output.parent).free < DISK_FLOOR:
        raise RuntimeError('At least 2 GiB free disk is required')
    bindgen = args.bindgen.resolve(strict=True)
    if digest(bindgen) != args.bindgen_sha256:
        raise RuntimeError('wasm-bindgen executable pin differs')
    # Keep only tool-discovery and locale settings. Ignore ambient compiler flags.
    allowed = ('PATH','HOME','USER','LOGNAME','LANG','LC_ALL','CARGO_HOME','RUSTUP_HOME','SDKROOT','DEVELOPER_DIR')
    env = {k: os.environ[k] for k in allowed if k in os.environ}
    env.update({'CARGO_NET_OFFLINE':'true','CARGO_INCREMENTAL':'0','CARGO_BUILD_JOBS':'1','RUSTUP_TOOLCHAIN':TOOLCHAIN})
    if not args.cargo.is_file():
        raise RuntimeError('Cargo executable is missing')
    # Preserve the cargo proxy name. Resolving its rustup symlink changes dispatch.
    cargo = str(args.cargo.absolute())
    cargo_version = subprocess.check_output([cargo, '+1.88.0', '--version'], cwd=SDK, env=env, text=True).strip()
    if not cargo_version.startswith('cargo 1.88.0 '):
        raise RuntimeError('Cargo 1.88.0 is required')
    if subprocess.check_output([str(bindgen), '--version'],env=env,text=True).strip() != BINDGEN_VERSION:
        raise RuntimeError('wasm-bindgen 0.2.100 is required')
    args.output.mkdir(exist_ok=False)
    sources = source_manifest()
    write_json(args.output/'SOURCE_INPUTS.json', sources)
    outputs = []
    cleaned = []
    for index in range(1, args.repetitions+1):
        if source_manifest() != sources:
            raise RuntimeError('Source changed during build')
        directory = args.output/f'build-{index}'
        directory.mkdir()
        with tempfile.TemporaryDirectory(prefix='dyt-ordinary-wasm-') as temporary:
            target_dir = Path(temporary)/'target'
            build_env = {**env, 'CARGO_TARGET_DIR':str(target_dir)}
            run([cargo,'+1.88.0','build','--package','dytallix-ordinary-browser','--target',TARGET,'--release','--locked','--offline','-j','1'], build_env, directory/'cargo.log')
            raw = target_dir/TARGET/'release/dytallix_ordinary_browser.wasm'
            raw_hash = digest(raw)
            run([str(bindgen), str(raw), '--target','web','--out-name','ordinary_browser','--out-dir',str(directory/'generated')], env, directory/'bindgen.log')
            outputs.append({'index':index,'raw_wasm_sha256':raw_hash,'assets':{name:digest(directory/'generated'/name) for name in ASSETS}})
        cleaned.append(not Path(temporary).exists())
    if source_manifest() != sources or digest(bindgen) != args.bindgen_sha256:
        raise RuntimeError('Source or binding tool changed during build')
    reference = None
    if args.reference:
        reference = {name: {'reference_sha256':digest(args.reference/name), 'built_sha256':outputs[0]['assets'][name],
                           'equal':digest(args.reference/name)==outputs[0]['assets'][name]} for name in ASSETS}
    result = {'status':'BUILT','rust_toolchain':TOOLCHAIN,'cargo_version':cargo_version,'target':TARGET,
              'profile':'release','wasm_bindgen':BINDGEN_VERSION,'wasm_bindgen_sha256':args.bindgen_sha256,
              'source_root':str(SDK),'source_input_count':len(sources),'compiler_environment':{'incremental':False,'jobs':1,'network':'offline','ambient_rustflags_inherited':False},
              'fresh_target_builds':args.repetitions,'outputs':outputs,
              'repeated_assets_identical':len(outputs)>1 and all(o['assets']==outputs[0]['assets'] for o in outputs),
              'repeated_raw_wasm_identical':len(outputs)>1 and all(o['raw_wasm_sha256']==outputs[0]['raw_wasm_sha256'] for o in outputs),
              'reference_comparison':reference,'temporary_targets_removed':all(cleaned),'source_unchanged':True,
              'qualification':'Same host, toolchain and dependency cache. No independent-machine reproduction.',
              'production_accepted':False,'mainnet_status':'NO_GO'}
    write_json(args.output/'BUILD_RESULT.json',result)
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cargo',type=Path,required=True,help='Rustup-managed cargo executable')
    parser.add_argument('--bindgen',type=Path,required=True)
    parser.add_argument('--bindgen-sha256',required=True,help='Previously verified binary digest for this host')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--reference',type=Path)
    parser.add_argument('--repetitions',type=int,choices=(1,2),default=2)
    args=parser.parse_args(); args.output=args.output.absolute()
    result=build(args)
    print(json.dumps({'status':result['status'],'fresh_target_builds':result['fresh_target_builds'],
                      'repeated_assets_identical':result['repeated_assets_identical'],
                      'reference_assets_identical':all(v['equal'] for v in result['reference_comparison'].values()) if result['reference_comparison'] else None}))

if __name__=='__main__': main()
