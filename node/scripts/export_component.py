#!/usr/bin/env python3
"""Export reviewed component source. This is not a release or package publication."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
COMPONENTS = {'pqc': 'pqc-crypto', 'contracts': 'smart-contracts'}


def export(component, output):
    output = output.resolve()
    if output.is_relative_to(ROOT):
        raise ValueError('Export outside the authoritative repository')
    source = ROOT / COMPONENTS[component]
    paths = [source / 'Cargo.toml']
    for directory in ['src', 'tests', 'examples']:
        paths.extend(p for p in (source / directory).rglob('*') if p.is_file()
                     and 'target' not in p.relative_to(source).parts
                     and '__pycache__' not in p.relative_to(source).parts)
    for name in ['README.md', 'LICENSE']:
        if (source / name).is_file():
            paths.append(source / name)
    entries = {str(p.relative_to(source)): p.read_bytes() for p in sorted(paths)}
    # Retain the authoritative workspace lockfile as evidence, not a standalone lockfile.
    entries['WORKSPACE-Cargo.lock'] = (ROOT / 'Cargo.lock').read_bytes()
    manifest = {
        'schema_version': 1,
        'authority': 'DytallixHQ/dytallix-node',
        'component': COMPONENTS[component],
        'repository_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'working_tree_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT, text=True).strip()),
        'purpose': 'Source distribution and comparison; not a qualified standalone package',
        'sha256': {name: hashlib.sha256(data).hexdigest() for name, data in entries.items()},
    }
    entries['SOURCE-MANIFEST.json'] = (json.dumps(manifest, indent=2) + '\n').encode()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(output, 'w:gz') as archive:
        for name, data in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o644
            archive.addfile(info, io.BytesIO(data))
    return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--component', choices=COMPONENTS, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = export(args.component, args.output)
    print(f"Exported {result['component']}; verify SOURCE-MANIFEST.json before use.")
