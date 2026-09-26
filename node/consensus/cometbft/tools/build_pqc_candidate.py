#!/usr/bin/env python3
"""Build and inspect a local candidate. This command never approves a release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tags', choices=('dytallix_pqc_only','dytallix_pqc_only,dytallix_pqc_ipc'), default='dytallix_pqc_only')
    parser.add_argument('--output-dir', type=Path, required=True)
    args = parser.parse_args()
    module = Path(__file__).resolve().parents[1]
    output = args.output_dir.resolve()
    if output.exists():
        parser.error('output directory must not exist; keep each candidate separate')
    if shutil.disk_usage(module).free < 2 * 1024**3:
        parser.error('less than 2 GiB of free space')
    output.mkdir(mode=0o700, parents=True)
    binary = output / 'dytallix-pqc-engine'
    inventory = output / 'CRYPTO_INVENTORY.json'
    checker = module / 'tools/pqc_boundary/check_boundary.py'
    go = shutil.which('go')
    if not go:
        parser.error('Go toolchain not found')
    # Disable ambient build/workspace switches and dependency downloads.
    env = dict(os.environ, CGO_ENABLED='0', GOFLAGS='', GOWORK='off',
               GOTOOLCHAIN='local', GOPROXY='off', GOSUMDB='off')
    command = [go, 'build', '-mod=readonly', '-trimpath', '-buildvcs=false',
               '-tags='+args.tags, '-p=1', '-o', str(binary),
               './cmd/dytallix-pqc-engine']
    record = {'schema_version': 1, 'status': 'BUILD_FAILED',
              'launch_status': 'NO_GO', 'g35_status': 'NOT_GRANTED',
              'production_qualified': False, 'release_authorized': False,
              'scope': 'One local Go engine candidate; no release packaging or deployment',
              'build_command': command, 'checker_sha256': sha(checker),
              'builder_sha256': sha(Path(__file__))}
    try:
        with (output / 'BUILD.log').open('w') as log:
            result = subprocess.run(command, cwd=module, env=env, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=600)
        record['build_exit_code'] = result.returncode
        if result.returncode == 0:
            record['binary'] = {'sha256': sha(binary), 'bytes': binary.stat().st_size}
            check = [sys.executable, '-B', str(checker), '--module-dir', str(module),
                     '--binary', str(binary), '--profile', 'pqc-engine-v1',
                     '--tags', args.tags, '--rebuild',
                     '--expected-sha256', record['binary']['sha256'],
                     '--output', str(inventory)]
            with (output / 'CHECK.log').open('w') as log:
                result = subprocess.run(check, cwd=module, env=env, stdout=log,
                                        stderr=subprocess.STDOUT, timeout=1200)
            record['check_exit_code'] = result.returncode
            if inventory.is_file():
                details = json.loads(inventory.read_text())
                record['inventory_sha256'] = sha(inventory)
                record['errors'] = details.get('errors', [])
                checked = (result.returncode == 0 and
                           details.get('boundary_status') == 'KNOWN_CLASSICAL_EXCLUSIONS_CHECKED' and
                           details.get('rebuild', {}).get('byte_identical') is True and
                           sha(binary) == record['binary']['sha256'] and
                           sha(checker) == record['checker_sha256'])
                record['status'] = 'LOCAL_EXCLUSION_CHECKED' if checked else 'CLASSICAL_EXCLUSION_BLOCKED'
            else:
                record['status'] = 'INVENTORY_FAILED'
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        record['error'] = str(error)
    (output / 'CANDIDATE.json').write_text(json.dumps(record, indent=2) + '\n')
    print(json.dumps({key: record[key] for key in
                      ['status', 'launch_status', 'g35_status', 'release_authorized']}))
    return 0 if record['status'] == 'LOCAL_EXCLUSION_CHECKED' else 1


if __name__ == '__main__':
    sys.exit(main())
