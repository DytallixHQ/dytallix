#!/usr/bin/env python3
"""Run the node's signed-fixture tests (E04 gap 11, UPG-003).

These are the ignored tests that verify real SLH-DSA-SHAKE-256s signatures:
root genesis, emergency controls, upgrades, release handover and history
replay. The script builds three test-only tools from
consensus/root-authorization, signs the public fixtures with disposable keys,
and runs the tests in process:

- dytallix-root-verify-snapshot: the verifier without the owner guard. The
  node accepts this launch only in test builds.
- dytallix-fixture-sign: signs test artifacts with keys derived from one
  public byte, so they must never be trusted by a real chain.
- the root-authorization test binary, whose TestExportDevelopmentGenesis
  signs development genesis bundles.

The two-binary process tests (cross_binary_compat, release_handover_process)
need an owner parent and a hardened host and are not run here; the long
penalty test runs in release on its own.

    python3 scripts/run_signed_fixture_tests.py [--work NEW_DIR] [--tools DIR]

--tools uses prebuilt tools (named as in TOOLS) instead of building them,
for hosts without Go.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ROOT_AUTHORIZATION = ROOT / 'consensus/root-authorization'
TOOLS = {
    'fixture-sign': ['go', 'build', '-mod=readonly', '-o', '{out}', './cmd/dytallix-fixture-sign'],
    'root-verify-snapshot': ['go', 'build', '-mod=readonly', '-o', '{out}', './cmd/dytallix-root-verify-snapshot'],
    'root-test-signer': ['go', 'test', '-mod=readonly', '-c', '-o', '{out}', '.'],
}
ARTIFACT = b'public-emergency-fixture'
# Variable: (action, disposable key discriminator). The two emergency
# fixtures use different keys.
FIXTURES = {
    'DYT_EMERGENCY_PUBLIC_FIXTURE': ('emergency', 17),
    'DYT_UPGRADE_PUBLIC_FIXTURE': ('upgrade', 18),
    'DYT_EMERGENCY_V2_PUBLIC_FIXTURE': ('emergency', 19),
}
SKIP = ['cross_binary_compat', 'release_handover_process', 'ten_thousand']
# A test dropped by a rename or a filter must fail the run, not shrink it.
EXPECTED = 21


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


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--work', type=Path, help='new directory for tools and fixtures')
    parser.add_argument('--tools', type=Path, help='directory of prebuilt tools')
    args = parser.parse_args()
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
               DYT_EMERGENCY_TEST_SIGNER=str(built['fixture-sign']),
               DYT_UPGRADE_TEST_SIGNER=str(built['fixture-sign']),
               **{name: str(path) for name, path in fixtures(work, built['fixture-sign']).items()})
    command = ['cargo', 'test', '--locked', '-p', 'dytallix-fast-node', '--lib', '--', '--ignored']
    for name in SKIP:
        command += ['--skip', name]
    print('+', ' '.join(command), flush=True)
    process = subprocess.run(command, cwd=ROOT, env=env, stdout=subprocess.PIPE,
                             stderr=subprocess.STDOUT, text=True)
    sys.stdout.write(process.stdout)
    passed = [int(count) for count in re.findall(r'^test result: ok\. (\d+) passed', process.stdout, re.M)]
    if process.returncode != 0:
        raise SystemExit(f'signed-fixture tests failed (exit {process.returncode})')
    if sum(passed) != EXPECTED:
        raise SystemExit(f'expected {EXPECTED} signed-fixture tests, {sum(passed)} passed')
    print(f'signed-fixture tests: {EXPECTED} passed')


if __name__ == '__main__':
    main()
