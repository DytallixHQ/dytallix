#!/usr/bin/env python3
"""Check declared dependency boundaries for the extracted modules."""
import json
from pathlib import Path
import sys
import tomllib


def check(root):
    policy = json.loads((root / 'docs/architecture/module-policy.json').read_text())
    errors = []
    for name, rule in policy['modules'].items():
        manifest_path = root / rule['path'] / 'Cargo.toml'
        manifest = tomllib.loads(manifest_path.read_text())
        if manifest['package']['name'] != name:
            errors.append(f'{name}: package name changed')
        if manifest['package'].get('publish') is not False:
            errors.append(f'{name}: publication requires a release policy decision')
        for section, allowed in rule['allowed_dependencies'].items():
            actual = manifest.get(section, {})
            for dependency, specification in actual.items():
                package = specification.get('package', dependency) if isinstance(specification, dict) else dependency
                if package not in allowed:
                    errors.append(f'{name}: forbidden {section} edge to {package}')
                if isinstance(specification, dict) and specification.get('workspace'):
                    errors.append(f'{name}: inherited dependencies require explicit policy support')
                if isinstance(specification, dict) and 'path' in specification:
                    destination = (manifest_path.parent / specification['path']).resolve()
                    expected = policy['local_packages'].get(package)
                    if expected is None or destination != (root / expected).resolve():
                        errors.append(f'{name}: unexpected source path for {package}')
        if manifest.get('target'):
            errors.append(f'{name}: target-specific dependencies require explicit policy support')
    expected_paths = {rule['path'] for rule in policy['modules'].values()}
    actual_paths = {str(path.parent.relative_to(root)) for path in (root / 'crates').glob('*/Cargo.toml')}
    for path in sorted(actual_paths - expected_paths):
        errors.append(f'{path}: module has no dependency policy')
    return errors


if __name__ == '__main__':
    errors = check(Path(__file__).resolve().parents[1])
    if errors:
        print('\n'.join(errors))
        sys.exit(1)
    print('Module dependency boundaries match the approved extraction policy.')
