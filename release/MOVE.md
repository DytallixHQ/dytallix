# Moving mainnet/ to DytallixHQ/dytallix

Before the first release is tagged, `mainnet/` moves with its history from
exocognosis/dytallix into the public repository `DytallixHQ/dytallix`, where
it becomes the root and releases are published (P01, 5 October 2026; the
[E06 release](../launch/approvals/P01_E06_RELEASE_2026-10-05.json) and
[repository](../launch/approvals/P01_E06_REPOSITORY_2026-10-05.json)
approvals). The founder does the account steps. Everything else is prepared
here and can be rehearsed locally first.

## What is already in place

- **Workflows.** `mainnet/.github/workflows` holds the CI for the new root:
  the current workflows with the `mainnet/` prefix removed, release tags
  `v*`, and a DCO check (`dco.yml`). GitHub ignores them until they are at
  the root. CI here fails if they drift from the current workflows
  ([repository.py](repository.py) `render-workflows --check`).
- **License and contributions.** MIT OR Apache-2.0 (`LICENSE-MIT`,
  `LICENSE-APACHE`, and both texts in each component's `LICENSE`); outside
  contributions are signed off under the [DCO](../DCO)
  ([CONTRIBUTING.md](../CONTRIBUTING.md)); a root
  [SECURITY.md](../SECURITY.md).
- **Module path.** The root authorization Go module is
  `dytallix.local/consensus/root-authorization`, like the other local
  modules, instead of a path on a GitHub account the project does not own.
- **History.** A scan of every file version in mainnet/'s history found no
  keys, tokens or personal addresses, only public test values. The commits'
  personal author address becomes the founder's GitHub noreply address
  during the move.

## Steps

1. **Freeze.** Merge or close the open pull requests that touch mainnet/,
   and merge nothing more there until step 6.
2. **Founder: organization and repository.**
   - Turn on two-factor authentication on your account, then require it for
     DytallixHQ (Settings, Authentication security).
   - Create the public repository `DytallixHQ/dytallix`, **empty**: no
     README, license or .gitignore.
   - In its Settings, Actions, General: allow only the actions the workflows
     pin (`actions/checkout`, `actions/setup-go`, `actions/upload-artifact`,
     `actions/download-artifact`, `dtolnay/rust-toolchain`,
     `Swatinem/rust-cache`); set workflow permissions to read repository
     contents; do not let Actions create or approve pull requests; require
     approval before running workflows from outside collaborators.
3. **Extract.** With git-filter-repo installed, from a checkout of
   exocognosis/dytallix at the frozen commit:

   ```text
   release/move.sh FROZEN_COMMIT WORK_DIR OLD_EMAIL
   ```

   It clones exocognosis/dytallix, keeps only main at FROZEN_COMMIT, makes
   mainnet/ the root with its history, replaces OLD_EMAIL (your personal
   commit address, given only on the command line) with
   `24476447+exocognosis@users.noreply.github.com`, and verifies the result:
   the new tree is exactly mainnet/ at FROZEN_COMMIT, and no commit names
   OLD_EMAIL. It prints `"status": "VERIFIED"` and pushes nothing.
4. **Push.** Review `WORK_DIR/dytallix`, then:

   ```text
   git -C WORK_DIR/dytallix remote add origin https://github.com/DytallixHQ/dytallix.git
   git -C WORK_DIR/dytallix push -u origin main
   ```

5. **Founder: repository settings.**
   - Add a ruleset on `main`: require a pull request (no approvals needed
     while you work alone), require the **DCO sign-off** status check, block
     force pushes and deletion. The build checks skip documentation-only
     changes, so they are not required; merge only green pull requests.
   - Under Security: turn on private vulnerability reporting, Dependabot
     alerts, and secret scanning with push protection.
   - Set the description and website (https://dytallix.com).
6. **Check CI there.** The push runs both workflows. The release build of
   FROZEN_COMMIT gives the same `SHA256SUMS` as this repository's build of
   that commit, since the builder sees the same tree at the same path.
7. **Point the old copy at the new home.** One pull request in
   exocognosis/dytallix replaces mainnet/ with a README pointing to
   DytallixHQ/dytallix and removes `mainnet.yml` and `mainnet-release.yml`
   from its workflows. Its history keeps everything.
8. **Afterwards, in DytallixHQ/dytallix.** Clone it fresh and commit with
   the noreply address. Update what still names the old homes: the SDK
   vendor records' `source_repository`
   ([sync_protocol_vendor.py](../sdk/scripts/sync_protocol_vendor.py)), the
   component README badges and links to the old separate repositories, and
   this file's past tense.

## Rehearsal

Steps 3 and 6 can be rehearsed with no account and no push: make a local
mirror and point the script at it.

```text
git clone --mirror . /tmp/dytallix-mirror.git
DYTALLIX_MOVE_SOURCE=/tmp/dytallix-mirror.git release/move.sh FROZEN_COMMIT /tmp/dytallix-move OLD_EMAIL
```

Then build the release from `/tmp/dytallix-move/dytallix` with
`release/reproduce.sh` and compare its `SHA256SUMS` with a build of
FROZEN_COMMIT here.
