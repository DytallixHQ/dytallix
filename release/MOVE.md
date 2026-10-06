# The move from exocognosis/dytallix

This repository began as the `mainnet/` folder of
[exocognosis/dytallix](https://github.com/exocognosis/dytallix). On 5 October
2026, before the first release tag, it moved here with its history (P01; the
[E06 release](../launch/approvals/P01_E06_RELEASE_2026-10-05.json) and
[repository](../launch/approvals/P01_E06_REPOSITORY_2026-10-05.json)
approvals). There, `mainnet/` is now a README pointing here.

## What moved

| | |
| --- | --- |
| Source | exocognosis/dytallix `31c391d08d0b576b2e427a4dcbb2fd7e32c303d1` (main, the merge of exocognosis/dytallix#346) |
| Result | `main` here at `ba131b655d9066a70ba97e9239a97e78fcf420b2`, 229 commits |
| Tree | `9a45a637c5f6c9f5bc72bb5fd55152e3898c7f40`, exactly `31c391d0:mainnet` |
| Identities | every commit's personal author and committer address became the founder's GitHub noreply address, `24476447+exocognosis@users.noreply.github.com` |

[move.sh](move.sh) made it with git-filter-repo: it kept only main at the
source commit, made `mainnet/` the root with its history, replaced the
personal address (given on the command line, never stored), and verified the
result with [repository.py](repository.py) `verify-move`. A scan of every file
version in the moved history found no keys, tokens or personal addresses, only
public test values. The release built from this repository is byte-identical
to the one built from the source commit.

## Checking it

Anyone can rebuild the history from exocognosis/dytallix and compare the
tree. Run from a checkout of this repository, with git-filter-repo
installed:

```text
release/move.sh 31c391d08d0b576b2e427a4dcbb2fd7e32c303d1 WORK_DIR OLD_EMAIL
git -C WORK_DIR/dytallix rev-parse HEAD^{tree}
```

The tree is `9a45a637…` whatever OLD_EMAIL is. With the founder's original
address the commit is `ba131b65…` too: a second run on 5 October gave the
same commit.

## Repository settings

The founder sets these in the browser:

- **Two-factor authentication** on the founder's account, and required for
  the DytallixHQ organization (Settings, Authentication security).
- **Actions** (Settings, Actions, General): allow actions created by GitHub
  plus `dtolnay/rust-toolchain@*` and `Swatinem/rust-cache@*`, which are the
  only others the workflows pin; require approval for workflows from outside
  contributors. Workflow tokens are read-only and cannot approve pull
  requests (already the defaults).
- **A ruleset on `main`**: require a pull request (no approvals needed while
  the founder works alone), require the **DCO sign-off** check (it becomes
  selectable after it first runs), block force pushes and deletion. The
  build workflows skip documentation-only changes, so they are not required
  checks; merge only green pull requests.
- **Security**: private vulnerability reporting, Dependabot alerts, and
  secret scanning with push protection.
