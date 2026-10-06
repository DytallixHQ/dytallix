#!/usr/bin/env python3
"""The move's verification and the contribution check (E06).

  repository.py verify-move NEW_REPO SOURCE_REPO SOURCE_COMMIT OLD_EMAIL
  repository.py check-dco BASE HEAD

`verify-move` checks a repository extracted by release/move.sh: its tree is
exactly mainnet/ at the source commit in exocognosis/dytallix, and no commit
carries the old author address (release/MOVE.md). `check-dco` checks that
every commit in BASE..HEAD is signed off by its author under the Developer
Certificate of Origin (DCO), which outside contributions need (P01,
5 October 2026; .github/workflows/dco.yml).
"""
import argparse
import json
import re
import subprocess

SIGN_OFF = re.compile(r'^Signed-off-by: (.+) <([^<>\s]+)>\s*$', re.M)


def git(repo, *args):
    return subprocess.run(['git', '-C', str(repo), *args], capture_output=True, text=True, check=True).stdout


def verify_move(new, source, commit, old_email):
    """Problems with an extracted repository; none means it is the move."""
    problems = []
    expected = git(source, 'rev-parse', f'{commit}:mainnet').strip()
    actual = git(new, 'rev-parse', 'HEAD^{tree}').strip()
    if actual != expected:
        problems.append(f'tree {actual} is not mainnet/ at {commit} ({expected})')
    old = old_email.strip().lower()
    for line in git(new, 'log', '--format=%H %ae %ce').splitlines():
        sha, author, committer = line.split()
        if old in (author.lower(), committer.lower()):
            problems.append(f'{sha} still names the old address')
    if old in git(new, 'log', '--format=%B').lower():
        problems.append('a commit message still names the old address')
    return problems


def check_dco(repo, base, head):
    """Commits in base..head without their author's sign-off."""
    missing = []
    for sha in git(repo, 'rev-list', '--no-merges', f'{base}..{head}').split():
        email = git(repo, 'log', '-1', '--format=%ae', sha).strip().lower()
        body = git(repo, 'log', '-1', '--format=%B', sha)
        if not any(e.lower() == email for _, e in SIGN_OFF.findall(body)):
            missing.append(sha)
    return missing


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest='command', required=True)
    v = commands.add_parser('verify-move')
    for name in ('new', 'source', 'commit', 'old_email'): v.add_argument(name)
    d = commands.add_parser('check-dco')
    d.add_argument('base')
    d.add_argument('head')
    args = parser.parse_args()
    if args.command == 'verify-move':
        problems = verify_move(args.new, args.source, args.commit, args.old_email)
        count = int(git(args.new, 'rev-list', '--count', 'HEAD'))
        print(json.dumps({'status': 'VERIFIED' if not problems else 'REFUSED', 'commits': count,
                          'tree': git(args.new, 'rev-parse', 'HEAD^{tree}').strip(), 'problems': problems}))
        return 1 if problems else 0
    missing = check_dco('.', args.base, args.head)
    for sha in missing:
        print(f'::error::{sha} has no Signed-off-by line with its author address (see CONTRIBUTING.md)')
    print(json.dumps({'status': 'SIGNED_OFF' if not missing else 'MISSING_SIGN_OFF', 'commits': missing}))
    return 1 if missing else 0


if __name__ == '__main__': raise SystemExit(main())
