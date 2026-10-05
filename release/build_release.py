#!/usr/bin/env python3
"""Build the Dytallix release set reproducibly, or compare two builds (E06).

  build_release.py build --src /src --out /out --work /work [--offline]
  build_release.py compare A/SHA256SUMS B/SHA256SUMS

`build` runs inside the pinned builder (builder/Dockerfile) with the mainnet
tree at --src. It builds every member in RELEASE_SET.json for Linux x86_64
and writes, under --out:

  bin/                the release binaries
  SHA256SUMS, SHA512SUMS
  BUILD_RECORD.json   toolchain versions, lockfile and migration registry
                      digests, each member's size and digests
  SBOM.json           every Rust crate and Go module the builds lock

Every member is a static executable: the Rust builds link glibc and
libstdc++ in, and Go builds without cgo (P01, 5 October 2026). A member
that would load a shared library fails the build. Nothing carries a time,
a host name or a builder path, so two builds in the builder give the same
bytes. `compare` exits 1 when two builds differ.

The release's authority is the root-signed genesis, which binds the release
manifest's SHA-512 (P01, 5 October 2026); these files are its inputs.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tomllib

SET_SCHEMA = 'dytallix.release-set.v1'
RECORD_SCHEMA = 'dytallix.release-build-record.v1'
SBOM_SCHEMA = 'dytallix.sbom.v1'
HERE = Path(__file__).resolve().parent
# Source and registry paths are fixed in the builder, and remapped out of
# the binaries' panic locations and debug data.
REMAPS = ('--remap-path-prefix=/src=/dytallix', '--remap-path-prefix=/usr/local/cargo/registry/src=/cargo')
# An explicit target keeps static linking off build scripts and proc macros.
RUST_TARGET = 'x86_64-unknown-linux-gnu'
RUSTFLAGS = ('-C', 'target-feature=+crt-static') + REMAPS
# Compiled into the node (upgrade.rs registry_sha256); the manifest names it.
MIGRATION_REGISTRY = 'node/dytallix-fast-launch/node/src/upgrade/registry.json'
NAME = re.compile(r'^[A-Za-z0-9_][A-Za-z0-9_.-]{0,63}$')


def load_set(path):
    """The release set, checked: unique binaries, and every catalog role built."""
    data = json.loads(Path(path).read_text())
    if data.get('schema') != SET_SCHEMA: raise ValueError('release set schema mismatch')
    if data['target'] != {'os': 'linux', 'arch': 'x86_64', 'abi': 'gnu'}:
        raise ValueError('the approved target is Linux x86_64 (P01, 5 October 2026)')
    names = []
    for build in data['builds']:
        if build['kind'] == 'cargo':
            names += build['bins']
        elif build['kind'] == 'go':
            names += [package.rsplit('/', 1)[-1] for package in build['packages']]
        else:
            raise ValueError(f'{build["id"]}: unknown build kind')
    if len(names) != len(set(names)) or not all(NAME.fullmatch(n) for n in names):
        raise ValueError('release binaries need distinct plain names')
    missing = sorted(set(data['catalog_roles'].values()) - set(names))
    if missing: raise ValueError(f'catalog roles name unbuilt binaries: {missing}')
    return data, names


def digests(path):
    raw = Path(path).read_bytes()
    return {'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest(), 'sha512': hashlib.sha512(raw).hexdigest()}


def sums(members, algorithm):
    """A sorted coreutils-style checksum file over bin/NAME."""
    return ''.join(f'{members[name][algorithm]}  bin/{name}\n' for name in sorted(members))


def parse_sums(text):
    entries = {}
    for line in text.splitlines():
        digest, _, path = line.partition('  ')
        if not re.fullmatch(r'[0-9a-f]{64}|[0-9a-f]{128}', digest) or not path or path in entries:
            raise ValueError(f'malformed checksum line: {line!r}')
        entries[path] = digest
    return entries


def compare(a, b):
    """Differences between two checksum files, as messages; none means identical."""
    left, right = parse_sums(Path(a).read_text()), parse_sums(Path(b).read_text())
    found = [f'only in {a}: {p}' for p in sorted(set(left) - set(right))]
    found += [f'only in {b}: {p}' for p in sorted(set(right) - set(left))]
    found += [f'differs: {p}' for p in sorted(set(left) & set(right)) if left[p] != right[p]]
    return found


def cargo_packages(lockfile):
    data = tomllib.loads(Path(lockfile).read_text())
    return [{'name': p['name'], 'version': p['version'], 'source': p.get('source', 'workspace'),
             'checksum': p.get('checksum')} for p in data['package']]


def go_modules(sumfile):
    """Module zips from go.sum (the /go.mod entries only pin metadata)."""
    modules = []
    for line in Path(sumfile).read_text().splitlines():
        module, version, digest = line.split()
        if not version.endswith('/go.mod'):
            modules.append({'module': module, 'version': version, 'h1': digest})
    return modules


def sbom(src, data):
    workspaces = sorted({b['workspace'] for b in data['builds'] if b['kind'] == 'cargo'})
    modules = sorted({b['module'] for b in data['builds'] if b['kind'] == 'go'})
    return {
        'schema': SBOM_SCHEMA,
        'cargo': {w: sorted(cargo_packages(src/w/'Cargo.lock'), key=lambda p: (p['name'], p['version'], p['source'])) for w in workspaces},
        'go': {m: sorted(go_modules(src/m/'go.sum'), key=lambda p: (p['module'], p['version'])) for m in modules},
    }


def needed_libraries(path):
    """The shared libraries an ELF binary loads (DT_NEEDED), or none if static."""
    out = subprocess.run(['readelf', '-d', str(path)], capture_output=True, text=True, check=True).stdout
    return sorted(re.findall(r'\(NEEDED\)\s+Shared library: \[([^\]]+)\]', out))


def check_static(path):
    """A release member loads no shared library and names no program interpreter."""
    segments = subprocess.run(['readelf', '-lW', str(path)], capture_output=True, text=True, check=True).stdout
    needed = needed_libraries(path)
    if needed or re.search(r'^\s*INTERP\s', segments, re.M):
        raise ValueError(f'{Path(path).name} is not static: interpreter or libraries {needed}')


def first_line(command):
    return subprocess.run(command, capture_output=True, text=True, check=True).stdout.splitlines()[0].strip()


def toolchain():
    return {
        'rustc': subprocess.run(['rustc', '-vV'], capture_output=True, text=True, check=True).stdout.strip().splitlines(),
        'cargo': first_line(['cargo', '-V']),
        'go': first_line(['go', 'version']),
        'clang': first_line(['clang', '--version']),
        'gcc': first_line(['gcc', '--version']),
        'ld': first_line(['ld', '--version']),
    }


def run(command, cwd, env):
    print('+ ' + ' '.join(command), file=sys.stderr, flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def check_rust_pin(src, data):
    """Every Rust workspace pins the builder's toolchain version."""
    pinned = os.environ.get('RUSTUP_TOOLCHAIN', '')
    for workspace in sorted({b['workspace'] for b in data['builds'] if b['kind'] == 'cargo'}):
        path = next((p/'rust-toolchain.toml' for p in [src/workspace, *(src/workspace).parents] if (p/'rust-toolchain.toml').is_file()), None)
        channel = tomllib.loads(path.read_text())['toolchain']['channel'] if path else None
        if not channel or not pinned.startswith(channel + '-'):
            raise ValueError(f'{workspace}: rust-toolchain.toml pins {channel}, the builder runs {pinned or "none"}')


def build(src, out, work, offline, set_path):
    data, names = load_set(set_path)
    check_rust_pin(src, data)
    if out.exists() and any(out.iterdir()): raise ValueError(f'{out} must be empty')
    (out/'bin').mkdir(parents=True, exist_ok=True)
    base = dict(os.environ, CARGO_INCREMENTAL='0', SOURCE_DATE_EPOCH='0', TZ='UTC')
    for b in data['builds']:
        env = dict(base)
        if b['kind'] == 'cargo':
            target = work/'cargo'/b['workspace'].replace('/', '_')
            env.update(CARGO_TARGET_DIR=str(target), RUSTFLAGS=' '.join(RUSTFLAGS))
            command = ['cargo', 'build', '--locked', '--release', '--target', RUST_TARGET]
            command += (['--offline'] if offline else []) + b['args']
            command += [arg for name in b['bins'] for arg in ('--bin', name)]
            run(command, src/b['workspace'], env)
            for name in b['bins']:
                shutil.copyfile(target/RUST_TARGET/'release'/name, out/'bin'/name)
        else:
            env.update(GOOS='linux', GOARCH='amd64', CGO_ENABLED='0', GOCACHE=str(work/'gocache'))
            if offline: env.update(GOPROXY='off', GOFLAGS='-mod=readonly')
            command = ['go', 'build', '-trimpath', '-buildvcs=false', '-ldflags=-buildid=', '-o', str(out/'bin') + '/']
            if b['tags']: command += ['-tags', ','.join(b['tags'])]
            run(command + b['packages'], src/b['module'], env)
    members = {}
    for name in names:
        path = out/'bin'/name
        path.chmod(0o755)
        check_static(path)
        members[name] = digests(path)
    lockfiles = sorted({f'{b["workspace"]}/Cargo.lock' for b in data['builds'] if b['kind'] == 'cargo'}
                       | {f'{b["module"]}/{f}' for b in data['builds'] if b['kind'] == 'go' for f in ('go.mod', 'go.sum')})
    record = {
        'schema': RECORD_SCHEMA,
        'release_set_sha256': hashlib.sha256(Path(set_path).read_bytes()).hexdigest(),
        'target': data['target'],
        'linkage': 'static',
        'toolchain': toolchain(),
        'lockfiles': {f: hashlib.sha256((src/f).read_bytes()).hexdigest() for f in lockfiles},
        'migration_registry_sha256': hashlib.sha256((src/MIGRATION_REGISTRY).read_bytes()).hexdigest(),
        'members': {name: members[name] for name in sorted(members)},
        'catalog_roles': data['catalog_roles'],
    }
    (out/'SHA256SUMS').write_text(sums(members, 'sha256'))
    (out/'SHA512SUMS').write_text(sums(members, 'sha512'))
    (out/'BUILD_RECORD.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
    (out/'SBOM.json').write_text(json.dumps(sbom(src, data), indent=2, sort_keys=True) + '\n')
    print(json.dumps({'status': 'BUILT', 'members': len(members),
                      'sha256sums_sha256': hashlib.sha256((out/'SHA256SUMS').read_bytes()).hexdigest()}))


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    b = commands.add_parser('build', help='build the release set (inside the builder)')
    b.add_argument('--src', type=Path, required=True, help='the mainnet tree')
    b.add_argument('--out', type=Path, required=True)
    b.add_argument('--work', type=Path, required=True, help='build caches, outside --out')
    b.add_argument('--offline', action='store_true', help='use only the local crate and module caches')
    b.add_argument('--set', type=Path, default=HERE/'RELEASE_SET.json')
    c = commands.add_parser('compare', help='compare two checksum files')
    c.add_argument('a', type=Path)
    c.add_argument('b', type=Path)
    args = parser.parse_args()
    if args.command == 'compare':
        found = compare(args.a, args.b)
        print(json.dumps({'status': 'IDENTICAL' if not found else 'DIFFERENT', 'differences': found}))
        return 0 if not found else 1
    build(args.src.resolve(), args.out, args.work, args.offline, args.set)
    return 0


if __name__ == '__main__': raise SystemExit(main())
