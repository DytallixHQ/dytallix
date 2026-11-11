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
# Lists fetched from a fallback mirror are read through its sources file,
# kept with them.
cached=()
if [ -f "$DIR/sources/ubuntu.sources" ]; then
  cached=(-o "Dir::Etc::SourceList=/dev/null" -o "Dir::Etc::SourceParts=$DIR/sources")
fi
if compgen -G "$lists/*_Packages*" > /dev/null \
    && "${apt[@]}" "${cached[@]}" install -y --no-download "${packages[@]}"; then
  echo "Installed ${packages[*]} from the cache; no mirror needed"
  exit 0
fi

resolves() {
  local host
  for host in "$@"; do
    timeout 10 getent hosts "$host" > /dev/null && return 0
  done
  return 1
}
online() {
  # A mirror that serves no index fails here, not at "Unable to locate".
  "${apt[@]}" "$@" update --error-on=any && "${apt[@]}" "$@" install -y "${packages[@]}"
}

# Canonical's mirrors, as GitHub's runners list them (/etc/apt/apt-mirrors.txt).
if resolves azure.archive.ubuntu.com archive.ubuntu.com security.ubuntu.com && online; then
  rm -f "$DIR/sources/ubuntu.sources"
  echo fetched=true >> "$GITHUB_OUTPUT"
  exit 0
fi

# Canonical's mirrors failed (outage, 11 November 2026: their DNS). Mirrors
# outside Canonical, checked current that day, serve the same signed
# archive: apt accepts their lists only under Ubuntu's archive key, so a
# mirror can be stale but cannot change a package. They carry amd64 only.
arch=$(dpkg --print-architecture)
if [ "$arch" = amd64 ]; then
  # shellcheck source=/dev/null
  codename=$(. /etc/os-release && echo "$VERSION_CODENAME")
  sources=$(mktemp -d)
  for mirror in https://mirrors.edge.kernel.org/ubuntu https://mirror.math.princeton.edu/pub/ubuntu; do
    host=${mirror#https://}
    resolves "${host%%/*}" || continue
    cat > "$sources/ubuntu.sources" <<EOF
Types: deb
URIs: $mirror
Suites: $codename $codename-updates $codename-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
    if online -o "Dir::Etc::SourceList=/dev/null" -o "Dir::Etc::SourceParts=$sources"; then
      mkdir -p "$DIR/sources"
      cp "$sources/ubuntu.sources" "$DIR/sources/"
      echo "::warning title=Ubuntu fallback mirror::Canonical's mirrors failed; installed ${packages[*]} from $mirror (signed by Ubuntu's archive key)."
      echo fetched=true >> "$GITHUB_OUTPUT"
      exit 0
    fi
  done
fi
echo "::error title=Ubuntu mirrors unreachable::Neither Canonical's apt mirrors nor the fallback mirrors served ${packages[*]}, and no cached packages fit: an upstream outage, not this change. Check status.canonical.com and rerun once it clears."
exit 1
