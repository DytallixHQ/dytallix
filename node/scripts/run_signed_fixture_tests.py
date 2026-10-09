#!/usr/bin/env python3
"""Run the node's signed-fixture tests (E04 gap 11, UPG-003).

These are the ignored tests that verify real SLH-DSA-SHAKE-256s signatures:
root genesis, emergency controls, upgrades, release handover and history
replay. The script builds three test-only tools from
consensus/root-authorization and signs the public fixtures with disposable
keys:

- dytallix-root-verify-snapshot: the verifier without the owner guard. The
  node accepts this launch only in test builds.
- dytallix-fixture-sign: signs test artifacts with keys derived from one
  public byte, so they must never be trusted by a real chain.
- the root-authorization test binary, whose TestExportDevelopmentGenesis
  signs development genesis bundles.
- dytallix-root-sign: the offline signer for the root genesis signers
  (production activation v1, A2). The three-of-five test generates five
  disposable keys with it, signs with three and four, and combines them.

By default it runs the 26 in-process tests. With --production it runs the
three-of-five root genesis test on a production build instead. With
--process (Linux only) it runs the four process tests instead
(cross_binary_compat,
release_handover_process): it also builds the owner launcher and two
test builds of consensus_stdio with distinct bytes (feature
test-snapshot-verifier; the second at opt-level 1), and each application
runs as an owned child under no_new_privs and a seccomp filter:

- --process systemd runs the tests through `sudo systemd-run` with the
  production system call deny list (the CI runner).
- --process none runs them directly; this process must already have both,
  as in a container started with `--security-opt no-new-privileges`.

The long penalty test runs in release on its own.

    python3 scripts/run_signed_fixture_tests.py [--work NEW_DIR] [--tools DIR]
        [--process {systemd,none} | --production]

--tools uses prebuilt tools (named as in TOOLS) instead of building them,
for hosts without Go.
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
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ROOT_AUTHORIZATION = ROOT / 'consensus/root-authorization'
PACKAGE = ROOT / 'dytallix-fast-launch/node'
TOOLS = {
    'fixture-sign': ['go', 'build', '-mod=readonly', '-o', '{out}', './cmd/dytallix-fixture-sign'],
    'root-verify-snapshot': ['go', 'build', '-mod=readonly', '-o', '{out}', './cmd/dytallix-root-verify-snapshot'],
    'root-test-signer': ['go', 'test', '-mod=readonly', '-c', '-o', '{out}', '.'],
    'root-sign': ['go', 'build', '-mod=readonly', '-o', '{out}', './cmd/dytallix-root-sign'],
}
ARTIFACT = b'public-emergency-fixture'
# Variable: (action, disposable key discriminator). The two emergency
# fixtures use different keys.
FIXTURES = {
    'DYT_EMERGENCY_PUBLIC_FIXTURE': ('emergency', 17),
    'DYT_UPGRADE_PUBLIC_FIXTURE': ('upgrade', 18),
    'DYT_EMERGENCY_V2_PUBLIC_FIXTURE': ('emergency', 19),
}
PROCESS_TESTS = ['cross_binary_compat', 'release_handover_process']
SKIP = PROCESS_TESTS + ['ten_thousand']
# A test dropped by a rename or a filter must fail the run, not shrink it.
EXPECTED_IN_PROCESS = 28
EXPECTED_PROCESS = 4
# The signed tests a production build runs (production activation v1, A2).
PRODUCTION_TESTS = ['root_genesis::threshold::tests::threshold_genesis_signed_three_of_five']
# The second application build differs only in the node crate's optimization.
CANDIDATE_BUILD = ['--config', 'profile.dev.package.dytallix-fast-node.opt-level=1']


def run(command, cwd, env=None):
    print('+', ' '.join(str(part) for part in command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def tools(work, prebuilt):
    paths = {}
    for name, command in TOOLS.items():
        if prebuilt:
            path = (prebuilt / name).resolve()
            if not os.access(path, os.X_OK):
                raise SystemExit(f'{path} is not an executable tool')
        else:
            path = work / name
            run([part.replace('{out}', str(path)) for part in command], ROOT_AUTHORIZATION)
        paths[name] = path
    return paths


def fixtures(work, signer):
    artifact = work / 'artifact.bin'
    artifact.write_bytes(ARTIFACT)
    paths = {}
    for variable, (action, key) in FIXTURES.items():
        output = work / f'{variable.lower()}.json'
        run([signer, '--artifact', artifact, '--output', output, '--action', action,
             '--fixture-key', str(key)], work)
        paths[variable] = output
    return paths


def target_dir():
    metadata = subprocess.run(['cargo', 'metadata', '--locked', '--format-version', '1', '--no-deps'],
                              cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
    return Path(json.loads(metadata.stdout)['target_directory'])


def application(work, name, extra):
    run(['cargo', 'build', '--locked', '-p', 'dytallix-fast-node', '--features', 'test-snapshot-verifier',
         '--bin', 'consensus_stdio', *extra], ROOT)
    path = work / name
    shutil.copy2(target_dir() / 'debug/consensus_stdio', path)
    return path


def test_binary():
    command = ['cargo', 'test', '--locked', '-p', 'dytallix-fast-node', '--lib', '--no-run',
               '--message-format=json']
    print('+', ' '.join(command), flush=True)
    output = subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True).stdout
    for line in output.splitlines():
        message = json.loads(line)
        if (message.get('reason') == 'compiler-artifact' and message.get('executable')
                and message['target']['name'] == 'dytallix_fast_node' and message['profile']['test']):
            return Path(message['executable'])
    raise SystemExit('node library test binary not found')


def sandboxed(mode, command, env):
    """The command under no_new_privs and a seccomp filter."""
    if mode == 'none':
        status = Path('/proc/self/status').read_text()
        if not (re.search(r'^NoNewPrivs:\s+1$', status, re.M) and re.search(r'^Seccomp:\s+2$', status, re.M)):
            raise SystemExit('--process none needs no_new_privs and a seccomp filter already applied')
        return command
    sys.path.insert(0, str(ROOT / 'tools/native-execution-policy'))
    import render  # noqa: E402
    return ['sudo', 'systemd-run', '--wait', '--pipe', '--collect', '--quiet',
            '-p', 'NoNewPrivileges=yes', '-p', f'SystemCallFilter={render.SYSTEM_CALL_DENY}',
            '-p', 'SystemCallErrorNumber=EPERM', '-p', 'SystemCallArchitectures=native',
            '-p', f'WorkingDirectory={PACKAGE}',
            *[f'--setenv={name}={value}' for name, value in sorted(env.items())
              if name.startswith('DYT_')],
            *command]


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--work', type=Path, help='new directory for tools and fixtures')
    parser.add_argument('--tools', type=Path, help='directory of prebuilt tools')
    parser.add_argument('--process', choices=['systemd', 'none'],
                        help='run the process tests, with this sandbox (Linux)')
    parser.add_argument('--production', action='store_true',
                        help='run the three-of-five root genesis test on a production build')
    args = parser.parse_args()
    if args.process and args.production:
        raise SystemExit('--process and --production are separate runs')
    if args.process and not sys.platform.startswith('linux'):
        raise SystemExit('the process tests need the Linux owner protocol')
    if args.work:
        args.work.mkdir(mode=0o700)
        work = args.work.resolve()
    else:
        work = Path(tempfile.mkdtemp(prefix='dyt-signed-')).resolve()
    built = tools(work, args.tools)
    env = dict(os.environ,
               DYT_ROOT_VERIFIER=str(built['root-verify-snapshot']),
               DYT_EMERGENCY_VERIFIER=str(built['root-verify-snapshot']),
               DYT_ROOT_TEST_SIGNER=str(built['root-test-signer']),
               DYT_ROOT_SIGNER=str(built['root-sign']),
               DYT_EMERGENCY_TEST_SIGNER=str(built['fixture-sign']),
               DYT_UPGRADE_TEST_SIGNER=str(built['fixture-sign']),
               **{name: str(path) for name, path in fixtures(work, built['fixture-sign']).items()})
    if args.process:
        binary = test_binary()
        run(['cargo', 'build', '--locked', '-p', 'dytallix-release-runtime', '--features',
             'qualification-fixtures', '--bin', 'release-runtime-owned-launch'], ROOT)
        launcher = work / 'owned-launch'
        shutil.copy2(target_dir() / 'debug/release-runtime-owned-launch', launcher)
        baseline = application(work, 'app-baseline', [])
        candidate = application(work, 'app-candidate', CANDIDATE_BUILD)
        digests = {hashlib.sha256(path.read_bytes()).hexdigest() for path in (baseline, candidate)}
        if len(digests) != 2:
            raise SystemExit('the two application builds must have distinct bytes')
        env.update(DYT_OWNER_LAUNCHER=str(launcher),
                   DYT_BASELINE_APP=str(baseline), DYT_CANDIDATE_APP=str(candidate),
                   DYT_HANDOVER_SOURCE_APP=str(baseline), DYT_HANDOVER_TARGET_APP=str(candidate))
        command = sandboxed(args.process, [str(binary), '--ignored', *PROCESS_TESTS], env)
        cwd, expected, kind = PACKAGE, EXPECTED_PROCESS, 'process'
    elif args.production:
        command = ['cargo', 'test', '--locked', '-p', 'dytallix-fast-node', '--features', 'production',
                   '--lib', '--', '--ignored', '--exact', *PRODUCTION_TESTS]
        cwd, expected, kind = ROOT, len(PRODUCTION_TESTS), 'production'
    else:
        command = ['cargo', 'test', '--locked', '-p', 'dytallix-fast-node', '--lib', '--', '--ignored']
        for name in SKIP:
            command += ['--skip', name]
        cwd, expected, kind = ROOT, EXPECTED_IN_PROCESS, 'in-process'
    print('+', ' '.join(command), flush=True)
    process = subprocess.run(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True)
    sys.stdout.write(process.stdout)
    passed = [int(count) for count in re.findall(r'^test result: ok\. (\d+) passed', process.stdout, re.M)]
    if process.returncode != 0:
        raise SystemExit(f'{kind} signed-fixture tests failed (exit {process.returncode})')
    if sum(passed) != expected:
        raise SystemExit(f'expected {expected} {kind} signed-fixture tests, {sum(passed)} passed')
    print(f'{kind} signed-fixture tests: {expected} passed')


if __name__ == '__main__':
    main()
