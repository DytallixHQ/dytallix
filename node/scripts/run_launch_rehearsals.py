#!/usr/bin/env python3
"""Run local launch prerequisites and retain blocked launch scenarios."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
REGISTER = ROOT / 'docs/mainnet/launch-rehearsals.json'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def snapshot():
    paths = sorted(set(git('ls-files', '-c', '-o', '--exclude-standard', '-z').split(b'\0')) - {b''})
    files = {}
    for raw in paths:
        name = os.fsdecode(raw)
        path = ROOT / name
        if path.is_symlink():
            value = 'symlink:' + os.readlink(path)
        elif path.is_file():
            value = f'{path.stat().st_mode & 0o777:o}:' + digest(path.read_bytes())
        elif not path.exists():
            value = 'deleted'
        else:
            raise RuntimeError(f'Unsupported source entry: {name}')
        files[name] = value
    return {
        'head': git('rev-parse', 'HEAD').decode().strip(),
        'status': git('status', '--short').decode(),
        'files': files,
        'sha256': digest(json.dumps(files, sort_keys=True).encode()),
    }


def execute(command, path, timeout, tests=False):
    started = time.monotonic()
    status = 'failed'
    code = None
    with path.open('w') as log:
        try:
            env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_TERM_COLOR='never', PYTHONDONTWRITEBYTECODE='1')
            with subprocess.Popen(command, cwd=ROOT, env=env, stdout=log,
                                  stderr=subprocess.STDOUT, start_new_session=True) as process:
                try:
                    code = process.wait(timeout=timeout)
                    status = 'passed' if code == 0 else 'failed'
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                    status = 'timed_out'
                except BaseException:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                    raise
        except OSError as error:
            log.write(f'Cannot execute check: {error}\n')
    body = path.read_bytes()
    passed_tests = sum(int(n) for n in re.findall(rb'test result: ok\. (\d+) passed;', body))
    if tests and status == 'passed' and passed_tests == 0:
        status = 'failed_no_tests'
    return {'command': command, 'status': status, 'exit_code': code,
            'seconds': round(time.monotonic() - started, 3), 'passed_tests': passed_tests,
            'log': path.name, 'log_sha256': digest(body)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['preflight', 'components'], default='preflight')
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    if output == ROOT or ROOT in output.parents:
        parser.error('Use an output directory outside the source repository')
    register = json.loads(REGISTER.read_text())
    if register['schema_version'] != 1:
        parser.error('Unsupported register version')
    expected_ids = [f'L{index:02}' for index in range(1, 21)]
    if [item.get('id') for item in register['scenarios']] != expected_ids:
        parser.error('The register must retain all 20 scenarios in order')
    if any(item.get('mandatory') is not True for item in register['scenarios']):
        parser.error('Every registered launch scenario must remain mandatory')
    output.mkdir(parents=True, exist_ok=False)
    before = snapshot()
    report = {
        'schema_version': 1, 'started_at': datetime.now(timezone.utc).isoformat(),
        'profile': args.profile, 'scope': 'local prerequisites only',
        'mainnet_ready': False, 'launch_suite_status': 'blocked',
        'system': platform.platform(), 'python': sys.version,
        'register_sha256': digest(REGISTER.read_bytes()), 'source_before': before,
        'scenarios': [],
    }
    local_commands = {
        'L01': [([sys.executable, '-B', 'scripts/adaptive_reference.py'], 60, False),
                ([sys.executable, '-B', 'scripts/address_reference.py'], 60, False),
                ([sys.executable, '-B', 'scripts/check_module_boundaries.py'], 60, False)],
        'L02': [(['cargo', 'test', '--offline', '--locked', '-p', package], 900, True)
                for package in ['dytallix-adaptive-emission', 'dytallix-protocol-types', 'dytallix-storage']],
    }
    selected = {'L01', 'L02'} if args.profile == 'components' else {'L01'}
    report_path = output / 'report.json'

    def save():
        temporary = output / 'report.tmp'
        temporary.write_text(json.dumps(report, indent=2) + '\n')
        temporary.replace(report_path)

    # Persist all unexecuted requirements before starting a process.
    for item in register['scenarios']:
        row = dict(item)
        row['status'] = 'not_run' if item['id'] in local_commands else 'blocked'
        row['checks'] = []
        report['scenarios'].append(row)
    save()
    report['tools'] = [execute(command, output / f'tool-{index}.log', 30)
                       for index, command in enumerate([['rustc', '-Vv'], ['cargo', '-V']])]
    for row in report['scenarios']:
        if row['id'] not in selected:
            continue
        for index, (command, timeout, tests) in enumerate(local_commands[row['id']]):
            result = execute(command, output / f'{row["id"]}-{index}.log', timeout, tests)
            row['checks'].append(result)
            save()
        row['status'] = 'passed' if all(check['status'] == 'passed' for check in row['checks']) else 'failed'
        print(f'{row["id"]}: {row["status"]}', flush=True)
        save()
    after = snapshot()
    report['source_after'] = after
    report['source_unchanged'] = before == after
    report['finished_at'] = datetime.now(timezone.utc).isoformat()
    selected_passed = all(row['status'] == 'passed' for row in report['scenarios'] if row['id'] in selected)
    tools_passed = all(item['status'] == 'passed' for item in report['tools'])
    report['local_checks_passed'] = selected_passed and tools_passed and report['source_unchanged']
    report['counts'] = {status: sum(row['status'] == status for row in report['scenarios'])
                        for status in ['passed', 'failed', 'blocked', 'not_run']}
    save()
    print(json.dumps(report['counts']))
    print(f'Launch suite: blocked. Mainnet ready: false. Report: {report_path}')
    return 0 if report['local_checks_passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
