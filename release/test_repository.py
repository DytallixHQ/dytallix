"""The repository move tools (E06), on throwaway git repositories."""
from pathlib import Path
import subprocess
import tempfile
import unittest
import repository as r

SOURCE = """on:
  push:
    tags: ['mainnet-v*']
    paths:
      - 'mainnet/**'
      - '!mainnet/**/*.md'
      - '.github/workflows/mainnet.yml'
jobs:
  node:
    defaults:
      run:
        working-directory: mainnet/node
    steps:
      # Built by mainnet/release/reproduce.sh; see tools/mainnet-preparation.
      - run: docker run -v "$GITHUB_WORKSPACE/mainnet:/src:ro" x mainnet/release/builder
      # move: drop-start
      - name: Old repository only
        run: python3 ../release/repository.py render-workflows --check
      # move: drop-end
      - run: (cd "mainnet/node/consensus/$module" && cat docs/mainnet/e01.md)
"""
RENDERED = """on:
  push:
    tags: ['v*']
    paths:
      - '**'
      - '!**/*.md'
      - '.github/workflows/mainnet.yml'
jobs:
  node:
    defaults:
      run:
        working-directory: node
    steps:
      # Built by release/reproduce.sh; see tools/mainnet-preparation.
      - run: docker run -v "$GITHUB_WORKSPACE:/src:ro" x release/builder
      - run: (cd "node/consensus/$module" && cat docs/mainnet/e01.md)
"""


def git(repo, *args, env=None):
    return subprocess.run(['git', '-C', str(repo), *args], capture_output=True, text=True, check=True,
                          env=env).stdout.strip()


class RenderTests(unittest.TestCase):
    def test_paths_lose_the_prefix_and_marked_steps_are_dropped(self):
        self.assertEqual(r.render(SOURCE), RENDERED)

    def test_markers_must_pair(self):
        for text in ('# move: drop-start\n', '# move: drop-end\n', '# move: drop-start\n# move: drop-start\n'):
            with self.assertRaises(ValueError): r.render(text)

    def test_check_reports_drift(self):
        with tempfile.TemporaryDirectory() as tmp:
            source, target = Path(tmp)/'source', Path(tmp)/'target'
            source.mkdir()
            for name in r.RENDERED: (source/name).write_text(SOURCE)
            self.assertEqual(r.render_workflows(True, source, target), list(r.RENDERED))
            self.assertEqual(r.render_workflows(False, source, target), [])
            self.assertEqual(r.render_workflows(True, source, target), [])
            (target/r.RENDERED[0]).write_text(RENDERED + '# edited\n')
            self.assertEqual(r.render_workflows(True, source, target), [r.RENDERED[0]])


class RepositoryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def repo(self, name):
        path = Path(self.tmp.name)/name
        path.mkdir()
        git(path, 'init', '-q', '-b', 'main')
        return path

    def commit(self, repo, message, email='dev@example.org', files=None):
        for name, text in (files or {'f': message}).items():
            (repo/name).parent.mkdir(parents=True, exist_ok=True)
            (repo/name).write_text(text)
        env = {'GIT_AUTHOR_NAME': 'Dev', 'GIT_AUTHOR_EMAIL': email, 'GIT_COMMITTER_NAME': 'Dev',
               'GIT_COMMITTER_EMAIL': email, 'HOME': self.tmp.name, 'PATH': '/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin'}
        git(repo, 'add', '-A', env=env)
        git(repo, 'commit', '-q', '-m', message, env=env)
        return git(repo, 'rev-parse', 'HEAD')

    def test_verify_move(self):
        source = self.repo('source')
        commit = self.commit(source, 'one', files={'mainnet/a': 'a', 'mainnet/node/b': 'b', 'testnet/c': 'c'})
        moved = self.repo('moved')
        self.commit(moved, 'one', email='1+founder@users.noreply.github.com', files={'a': 'a', 'node/b': 'b'})
        self.assertEqual(r.verify_move(moved, source, commit, 'old@example.org'), [])
        # A different tree, and a commit that keeps the old address.
        self.commit(moved, 'two', email='old@example.org', files={'node/b': 'changed'})
        problems = r.verify_move(moved, source, commit, 'OLD@example.org')
        self.assertTrue(any('is not mainnet/' in p for p in problems))
        self.assertTrue(any('old address' in p for p in problems))

    def test_check_dco(self):
        repo = self.repo('dco')
        base = self.commit(repo, 'base')
        signed = self.commit(repo, 'signed\n\nSigned-off-by: Dev <DEV@example.org>')
        self.assertEqual(r.check_dco(repo, base, signed), [])
        other = self.commit(repo, 'someone else\n\nSigned-off-by: Other <other@example.org>')
        plain = self.commit(repo, 'none')
        self.assertEqual(sorted(r.check_dco(repo, base, plain)), sorted([other, plain]))


if __name__ == '__main__': unittest.main(verbosity=2)
