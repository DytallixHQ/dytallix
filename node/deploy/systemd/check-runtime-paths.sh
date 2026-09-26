#!/bin/sh
# Check paths as the service account. Do not read or print key contents.
set -eu

fail() { printf '%s\n' "$1" >&2; exit 1; }
absolute() {
    case "$2" in /*) ;; *) fail "$1 must be an explicit absolute path" ;; esac
}

[ "${DYT_REQUIRE_EXISTING_VALIDATOR_KEY:-}" = 1 ] || fail 'Service requires read-only existing-key mode'

absolute DYT_DATA_DIR "${DYT_DATA_DIR:-}"
[ -d "$DYT_DATA_DIR" ] && [ -w "$DYT_DATA_DIR" ] || fail 'Data directory must exist and be writable'
[ -r ./genesis.json ] || fail 'genesis.json must be readable in the working directory'
[ -n "${VALIDATOR_ID:-}" ] || fail 'Set the existing VALIDATOR_ID explicitly'

vault_url=${DYTALLIX_VAULT_URL-${VAULT_URL-}}
vault_token=${DYTALLIX_VAULT_TOKEN-${VAULT_TOKEN-}}
if [ -n "$vault_url" ] || [ -n "$vault_token" ]; then
    [ -n "$vault_url" ] && [ -n "$vault_token" ] || fail 'Vault configuration requires both URL and token'
else
    absolute DYT_KEYSTORE_DIR "${DYT_KEYSTORE_DIR:-}"
    key_file="$DYT_KEYSTORE_DIR/validator-$VALIDATOR_ID.seal"
    [ -r "$key_file" ] && [ -s "$key_file" ] || fail 'Existing validator key must be readable and nonempty'
fi
