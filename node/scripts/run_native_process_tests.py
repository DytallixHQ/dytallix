#!/usr/bin/env python3
"""Run the native supervisor's Linux owned-process tests.

The supervisor owns its children through the release-runtime owner
protocol: it creates them only from the process leader thread, and admits
one only when it shows the exact security state its role expects (UID, GID,
mount namespace, an enforced AppArmor label, no_new_privs and a seccomp
filter). The cases live in `processes::qualification`; the runner
tests/process_qualification.rs runs each on the main thread of a fresh
process, since libtest runs tests on worker threads.

This script:

- builds the process fixture and the runner. The fixture has no debug
  information: the synthetic catalog bounds each member at 128 MiB.
- loads two AppArmor profiles in enforce mode. Each attaches by path to one
  copy of the fixture that a case makes under the work directory: the
  application copy, or the workload copy that the bridge, engine and adapter
  roles run. The profiles give each role an exact label; they allow what the
  fixture does and qualify no policy (E02 does).
- runs the runner as the invoking (non-root) user through `sudo systemd-run`,
  with no_new_privs and the production system call deny list.
- unloads the profiles and requires every case to pass.

    python3 scripts/run_native_process_tests.py [--work NEW_DIR]

Linux with AppArmor, systemd and sudo, such as the CI runner.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = 'dytallix-native-supervisor'
FEATURES = ['--features', 'qualification-fixtures']
# A case dropped from the runner must fail the run, not shrink it.
EXPECTED = 18
# The names in processes::qualification.
DIRECTORY_PREFIX = 'dyt-native-process-'
COPIES = {'application': 'DYT_NATIVE_PROCESS_APPLICATION_LABEL',
          'workload': 'DYT_NATIVE_PROCESS_WORKLOAD_LABEL'}
PROFILES = '''abi <abi/4.0>,
# Native process qualification only: an exact label for each copy of the
# fixture. They allow what the fixture does; E02 qualifies the real role policy.
'''
PROFILE = '''profile {name} {attachment} {{
  file,
  network,
  unix,
  signal,
  ptrace,
}}
'''


def run(command, cwd=ROOT, env=None, **kwargs):
    print('+', ' '.join(str(part) for part in command), flush=True)
    return subprocess.run(command, cwd=cwd, env=env, check=True, **kwargs)


def target_dir():
    metadata = run(['cargo', 'metadata', '--locked', '--format-version', '1', '--no-deps'],
                   stdout=subprocess.PIPE, text=True)
    return Path(json.loads(metadata.stdout)['target_directory'])


def build(work):
    """The fixture and the runner, in the test profile without debug information."""
    env = dict(os.environ, CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
    run(['cargo', 'build', '--locked', '--profile', 'test', '-p', PACKAGE, *FEATURES,
         '--bin', 'native-supervisor-process-fixture'], env=env)
    fixture = work / 'native-supervisor-process-fixture'
    shutil.copy2(target_dir() / 'debug/native-supervisor-process-fixture', fixture)
    output = run(['cargo', 'test', '--locked', '-p', PACKAGE, *FEATURES, '--test', 'process_qualification',
                  '--no-run', '--message-format=json'], env=env, stdout=subprocess.PIPE, text=True).stdout
    for line in output.splitlines():
        message = json.loads(line)
        if (message.get('reason') == 'compiler-artifact' and message.get('executable')
                and message['target']['name'] == 'process_qualification'):
            return fixture, Path(message['executable'])
    raise SystemExit('process_qualification runner not found')


def profiles(cases):
    """The profile text, and the label for each copy's variable."""
    labels = {variable: f'dyt-native-process-{copy}' for copy, variable in COPIES.items()}
    return PROFILES + ''.join(
        PROFILE.format(name=labels[variable], attachment=f'{cases}/{DIRECTORY_PREFIX}*/{copy}')
        for copy, variable in COPIES.items()), labels


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--work', type=Path, help='new directory for the fixture, profiles and cases')
    args = parser.parse_args()
    if not sys.platform.startswith('linux'):
        raise SystemExit('the process tests need Linux')
    if Path('/sys/module/apparmor/parameters/enabled').read_text().strip() != 'Y':
        raise SystemExit('the process tests need AppArmor enabled')
    if os.geteuid() == 0:
        raise SystemExit('run as the non-root user the cases run as; the script uses sudo')
    if args.work:
        args.work.mkdir(mode=0o700)
        work = args.work.resolve()
    else:
        work = Path(tempfile.mkdtemp(prefix='dyt-native-process-run-')).resolve()
    if re.search(r'[\s"#,{}\[\]*?^]', str(work)):
        raise SystemExit(f'{work}: the AppArmor attachments need a plain path')
    fixture, runner = build(work)
    cases = work / 'cases'
    cases.mkdir(mode=0o700)
    policy = work / 'native-process.apparmor'
    text, labels = profiles(cases)
    policy.write_text(text)
    run(['sudo', 'apparmor_parser', '-r', '-T', '-K', policy])
    sys.path.insert(0, str(ROOT / 'tools/native-execution-policy'))
    import render  # noqa: E402
    env = {'DYT_NATIVE_PROCESS_FIXTURE': fixture, 'TMPDIR': cases, **labels}
    command = ['sudo', 'systemd-run', '--wait', '--pipe', '--collect', '--quiet',
               '-p', f'User={os.getuid()}', '-p', f'Group={os.getgid()}',
               '-p', 'NoNewPrivileges=yes', '-p', f'SystemCallFilter={render.SYSTEM_CALL_DENY}',
               '-p', 'SystemCallErrorNumber=EPERM', '-p', 'SystemCallArchitectures=native',
               '-p', f'WorkingDirectory={work}',
               *[f'--setenv={name}={value}' for name, value in sorted(env.items())],
               runner]
    try:
        print('+', ' '.join(str(part) for part in command), flush=True)
        process = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    finally:
        run(['sudo', 'apparmor_parser', '-R', '-T', '-K', policy])
    sys.stdout.write(process.stdout)
    passed = [int(count) for count in re.findall(r'^test result: ok\. (\d+) passed', process.stdout, re.M)]
    if process.returncode != 0:
        raise SystemExit(f'native process tests failed (exit {process.returncode})')
    if sum(passed) != EXPECTED:
        raise SystemExit(f'expected {EXPECTED} native process tests, {sum(passed)} passed')
    print(f'native process tests: {EXPECTED} passed')


if __name__ == '__main__':
    main()
