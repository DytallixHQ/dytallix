#!/usr/bin/env python3
"""The move to DytallixHQ/dytallix, and its contribution check (E06).

  repository.py render-workflows [--check]
  repository.py verify-move NEW_REPO SOURCE_REPO SOURCE_COMMIT OLD_EMAIL
  repository.py check-dco BASE HEAD

mainnet/ moves with its history into DytallixHQ/dytallix before the first
release tag (P01, 5 October 2026; release/MOVE.md). There it is the
repository root, so its CI workflows live in mainnet/.github/workflows,
which GitHub ignores until the move. `render-workflows` writes them from
this repository's workflows with the mainnet/ prefix removed, and drops
the steps marked as belonging here; `--check` exits 1 when they differ.

`verify-move` checks an extracted repository (release/move.sh): its tree is
exactly mainnet/ at the source commit, and no commit carries the old author
address. `check-dco` checks that every commit in BASE..HEAD is signed off
by its author under the Developer Certificate of Origin (DCO), which
outside contributions need (P01, 5 October 2026).
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

HERE = Path(__file__).resolve().parent
MAINNET = HERE.parent
SOURCE_WORKFLOWS = MAINNET.parent/'.github'/'workflows'
TARGET_WORKFLOWS = MAINNET/'.github'/'workflows'
RENDERED = ('mainnet.yml', 'mainnet-release.yml')
DROP_START = '# move: drop-start'
DROP_END = '# move: drop-end'
PREFIX = re.compile(r'(?<![\w./-])mainnet/')
SIGN_OFF = re.compile(r'^Signed-off-by: (.+) <([^<>\s]+)>\s*$', re.M)


def render(text):
    """A workflow for the repository whose root is mainnet/."""
    lines, dropping = [], False
    for line in text.splitlines(keepends=True):
        marker = line.strip()
        if marker == DROP_START:
            if dropping: raise ValueError('nested drop marker')
            dropping = True
        elif marker == DROP_END:
            if not dropping: raise ValueError('unmatched drop marker')
            dropping = False
        elif not dropping:
            lines.append(line)
    if dropping: raise ValueError('unterminated drop marker')
    out = ''.join(lines).replace('$GITHUB_WORKSPACE/mainnet:', '$GITHUB_WORKSPACE:')
    out = out.replace("'mainnet-v*'", "'v*'")
    return PREFIX.sub('', out)


def render_workflows(check, source=SOURCE_WORKFLOWS, target=TARGET_WORKFLOWS):
    differ = []
    for name in RENDERED:
        rendered = render((source/name).read_text())
        path = target/name
        if check:
            if not path.is_file() or path.read_text() != rendered: differ.append(name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(rendered)
    return differ


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
    r = commands.add_parser('render-workflows')
    r.add_argument('--check', action='store_true')
    v = commands.add_parser('verify-move')
    for name in ('new', 'source', 'commit', 'old_email'): v.add_argument(name)
    d = commands.add_parser('check-dco')
    d.add_argument('base')
    d.add_argument('head')
    args = parser.parse_args()
    if args.command == 'render-workflows':
        differ = render_workflows(args.check)
        print(json.dumps({'status': 'DIFFERENT' if differ else ('IN_SYNC' if args.check else 'WRITTEN'),
                          'workflows': differ or list(RENDERED)}))
        return 1 if differ else 0
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
