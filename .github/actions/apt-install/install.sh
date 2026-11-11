#!/usr/bin/env bash
# Install $PACKAGES: offline from the restored cache in $DIR when it holds
# every package, else online once Ubuntu's mirrors resolve. Sets
# fetched=true in $GITHUB_OUTPUT after an online install, so the caller
# saves the cache.
set -euo pipefail
: "${PACKAGES:?set by action.yml}" "${DIR:?set by action.yml}"
read -r -a packages <<< "$PACKAGES"
archives="$DIR/archives"
lists="$DIR/lists"
mkdir -p "$archives/partial" "$lists/partial"
# apt downloads as _apt; hand the cache back to the runner for the save.
sudo chown _apt "$archives/partial" "$lists/partial"
trap 'sudo rm -f "$archives/lock" "$lists/lock"; sudo chown -R "$(id -u):$(id -g)" "$DIR"' EXIT
apt=(sudo apt-get -o "Dir::Cache::Archives=$archives" -o "Dir::State::Lists=$lists"
     -o Acquire::Retries=3 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)

# The lists were signature-checked when fetched; apt checks each .deb's
# hash against them. A package missing from the cache fails --no-download.
if compgen -G "$lists/*_Packages*" > /dev/null \
    && "${apt[@]}" install -y --no-download "${packages[@]}"; then
  echo "Installed ${packages[*]} from the cache; no mirror needed"
  exit 0
fi

# GitHub's runners list these mirrors (/etc/apt/apt-mirrors.txt).
resolved=false
for host in azure.archive.ubuntu.com archive.ubuntu.com security.ubuntu.com; do
  if timeout 10 getent hosts "$host" > /dev/null; then
    resolved=true
    break
  fi
done
if [ "$resolved" != true ]; then
  echo "::error title=Ubuntu mirrors unreachable::None of Ubuntu's apt mirrors resolves from this runner and no cached packages fit, so this is an upstream outage, not this change. Check status.canonical.com and rerun once it clears."
  exit 1
fi
"${apt[@]}" update
"${apt[@]}" install -y "${packages[@]}"
echo fetched=true >> "$GITHUB_OUTPUT"
