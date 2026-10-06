#!/usr/bin/env bash
# Extract mainnet/ with its history into a new local repository for
# DytallixHQ/dytallix (E06; release/MOVE.md). It pushes nothing.
#
#   release/move.sh SOURCE_COMMIT WORK_DIR OLD_EMAIL
#
# SOURCE_COMMIT is the exocognosis/dytallix main commit that moves. The
# history's author address OLD_EMAIL becomes the founder's GitHub noreply
# address (P01, 5 October 2026); it is an argument so that this repository
# never names it. DYTALLIX_MOVE_SOURCE overrides the source, for example a
# local mirror for a rehearsal. Needs git-filter-repo.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
usage='usage: release/move.sh SOURCE_COMMIT WORK_DIR OLD_EMAIL'
commit="${1:?$usage}"
work="${2:?$usage}"
old="${3:?$usage}"
source="${DYTALLIX_MOVE_SOURCE:-https://github.com/exocognosis/dytallix.git}"
noreply='24476447+exocognosis@users.noreply.github.com'
command -v git-filter-repo >/dev/null || { echo 'git-filter-repo is required' >&2; exit 2; }
if [ -e "$work" ]; then echo "$work must not exist" >&2; exit 2; fi
mkdir -p "$work"
work="$(cd "$work" && pwd)"

# A pristine copy to verify against, and the copy that is rewritten. Neither
# checks out the whole old repository.
git clone --quiet --no-local --no-tags --no-checkout "$source" "$work/source"
git -C "$work/source" cat-file -e "$commit^{commit}"
git clone --quiet --no-local --no-tags --no-checkout "$work/source" "$work/dytallix"
cd "$work/dytallix"
git update-ref refs/heads/main "$commit"
git symbolic-ref HEAD refs/heads/main
git remote remove origin
git for-each-ref --format='%(refname)' | while read -r ref; do
  [ "$ref" = refs/heads/main ] || git update-ref -d "$ref"
done

# mainnet/ becomes the root; only the address changes in each identity.
printf '<%s> <%s>\n' "$noreply" "$old" > "$work/mailmap"
git filter-repo --force --quiet --subdirectory-filter mainnet --mailmap "$work/mailmap"
rm "$work/mailmap"
git reset --quiet --hard
git config user.email "$noreply"

python3 "$here/repository.py" verify-move "$work/dytallix" "$work/source" "$commit" "$old"
cat <<EOF
Extracted to $work/dytallix. Review it, then push it to the new repository
(release/MOVE.md, step 4):
  git -C $work/dytallix remote add origin https://github.com/DytallixHQ/dytallix.git
  git -C $work/dytallix push -u origin main
EOF
