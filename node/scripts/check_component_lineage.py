#!/usr/bin/env python3
"""Check the reviewed embedded component inventory. This is not a security audit."""
import hashlib
import json
from pathlib import Path


def inventory(component):
    paths = [component / 'Cargo.toml']
    for directory in ['src', 'tests']:
        paths.extend(p for p in (component / directory).rglob('*') if p.is_file())
    return {
        str(p.relative_to(component)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(paths)
    }


def check(root):
    manifest = json.loads((root / 'docs/component-lineage.json').read_text())
    errors = []
    for component in manifest['components']:
        actual = inventory(root / component['embedded_path'])
        expected = component['embedded_sha256']
        for name in sorted(set(actual) | set(expected)):
            if actual.get(name) != expected.get(name):
                errors.append(f"{component['embedded_path']}/{name}: review and update the lineage record")
    return errors


if __name__ == '__main__':
    errors = check(Path(__file__).resolve().parents[1])
    if errors:
        print('\n'.join(errors))
        raise SystemExit(1)
    print('Embedded component inventory matches the recorded candidate.')
