#!/usr/bin/env python3
"""Build one host's bundle (E05; node/docs/architecture/host-setup-v1.md).

  host_bundle.py --host-files OUT/LABEL --release DIR --sealed LABEL.sealed.json --out LABEL.bundle.tar

Inputs, all public:
  --host-files  host_files.py's output for this host (INSTALL_MANIFEST.json
                and every file under its absolute path)
  --release     the release build's output (bin/ holds its binaries)
  --sealed      the host's node keys, sealed by `dytallix-root-sign seal`

The bundle is one deterministic tar archive, everything under
dytallix-host/: the install manifest, install.sh, wipe.sh and
host_install.py, the release binaries in bin/, the files in files/ and the
sealed keys. Every file is checked against the manifest first, and the
sealed keys must name exactly the manifest's secret files. It prints the
bundle's SHA-256, to note on paper for the console, and writes it beside
the bundle in sha256sum format.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import sys
import tarfile

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / 'host'))
import host_install  # noqa: E402

PREFIX = 'dytallix-host'
SCRIPTS = (('install.sh', 0o755), ('wipe.sh', 0o755), ('host_install.py', 0o644))


class Invalid(ValueError):
    pass


def entries(host_files, release, sealed):
    """The bundle's files, by archive path: (bytes, mode)."""
    host_files, release = Path(host_files), Path(release)
    files = {}
    bundle_view = {'INSTALL_MANIFEST.json': (host_files / 'INSTALL_MANIFEST.json').read_bytes(),
                   'keys.sealed.json': Path(sealed).read_bytes()}
    manifest = json.loads(bundle_view['INSTALL_MANIFEST.json'])
    for name, raw in bundle_view.items():
        files[f'{PREFIX}/{name}'] = (raw, 0o644)
    for name, mode in SCRIPTS:
        files[f'{PREFIX}/{name}'] = ((HERE / 'host' / name).read_bytes(), mode)
    for binary in manifest.get('binaries', []):
        files[f'{PREFIX}/bin/{binary["name"]}'] = ((release / 'bin' / binary['name']).read_bytes(), 0o755)
    for row in manifest.get('files', []):
        relative = PurePosixPath(row['path']).relative_to('/')
        files[f'{PREFIX}/files/{relative}'] = ((host_files / relative).read_bytes(), 0o644)
    return files


def check(files):
    """The installer's own bundle check, on the bundle's files."""
    try:
        return host_install.load_bundle(None, read=lambda relative: files[f'{PREFIX}/{relative}'][0])
    except host_install.Refused as error:
        raise Invalid(str(error)) from error


def archive(files):
    """A deterministic tar: sorted, root-owned, time zero."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w', format=tarfile.GNU_FORMAT) as tar:
        directories = sorted({str(parent) for path in files for parent in PurePosixPath(path).parents
                              if str(parent) != '.'})
        for directory in directories:
            info = tarfile.TarInfo(directory)
            info.type, info.mode, info.mtime = tarfile.DIRTYPE, 0o755, 0
            info.uid = info.gid = 0
            info.uname = info.gname = 'root'
            tar.addfile(info)
        for path in sorted(files):
            raw, mode = files[path]
            info = tarfile.TarInfo(path)
            info.size, info.mode, info.mtime = len(raw), mode, 0
            info.uid = info.gid = 0
            info.uname = info.gname = 'root'
            tar.addfile(info, io.BytesIO(raw))
    return buffer.getvalue()


def build(host_files, release, sealed, out):
    out = Path(out)
    if out.exists():
        raise Invalid(f'{out} exists; outputs are never overwritten')
    files = entries(host_files, release, sealed)
    manifest = check(files)
    raw = archive(files)
    digest = hashlib.sha256(raw).hexdigest()
    with open(out, 'xb') as file:
        file.write(raw)
    with open(out.with_name(out.name + '.sha256'), 'x') as file:
        file.write(f'{digest}  {out.name}\n')
    return {'status': 'BUILT', 'label': manifest['label'], 'role': manifest['role'], 'release': manifest['release'],
            'bundle': str(out), 'bytes': len(raw), 'sha256': digest}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for name in ('host-files', 'release', 'sealed', 'out'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    try:
        result = build(args.host_files, args.release, args.sealed, args.out)
    except (Invalid, OSError, KeyError, ValueError) as error:
        print(f'host_bundle: {error}', file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
