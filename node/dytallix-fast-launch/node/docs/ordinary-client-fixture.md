# Disposable ordinary client fixture

The active test `export_disposable_ordinary_client_fixture` validates a fresh local genesis through the real node. It does not import SDK code into the node.

The test performs these checks on every run:

1. Generate fresh ML-DSA-65 and ML-DSA-87 account keys.
2. Bind their stable addresses, recovery state, origin records and fee roles to one exact genesis digest.
3. Serialize and decode the configuration. Require at most 65,536 bytes, as required by `consensus_stdio`.
4. Call `ConsensusConfig::validate`, `ConsensusApplication::open` and `init_chain` with the exact serialized configuration and genesis bytes.
5. Close and reopen the database. Require the same application hash and ordinary profile query.

These checks use the full node genesis path. A client-side JSON or digest check alone does not replace them. The SDK runner must then test its own prepared and signed transactions through the actual `consensus_stdio` binary.

## Optional export

Set `DYTX_ORDINARY_CLIENT_FIXTURE_DIR` to an existing empty directory with an absolute path. Use a new disposable temporary directory. Without this variable, the test exports nothing. The test remains active and is not ignored.

The exporter rejects a non-directory, a final symlink, a relative path or a nonempty directory. It sets directory permissions to `0700`. It creates each file with `create_new` and permissions `0600`. It writes no key material to stdout. Unix private file permissions are required for export.

| File | Content |
| --- | --- |
| `config.json` | Compact, explicit local node configuration. |
| `genesis.json` | Exact genesis bytes whose SHA-256 digest appears in configuration and account domains. |
| `init-chain.json` | Complete `init_chain` pipe request, including exact base64 genesis, validators and evidence limits. |
| `profile.json` | Actual initialized-node `ProfileView` query. |
| `account-ROLE.json` | Actual initialized-node `AccountView` query. |
| `context-ROLE.json` | Caller-selected SDK signing context for that initial local state. This is not a consensus proof. |
| `actions.json` | A seven-uDRT transfer to the payer account. |
| `public-identities.json` | `accounts` array with role, account ID, address, algorithm and public key. |
| `secret-fixtures.json` | `accounts` array with role, account ID, algorithm, public key and private key. All keys are freshly generated disposable test keys. |
| `key-ROLE.json` | Strict CLI key file: algorithm, public-key byte array and private-key byte array. |
| `manifest.json` | Public hashes, algorithms and validation method. |

Roles are `active` (ML-DSA-87), `payer` (ML-DSA-87) and `secondary` (ML-DSA-65). The secondary replaces one disposable fixture account. The normal shared fixture is unchanged.

Start a separate `consensus_stdio` process with `--config`, `--genesis` and a new `--db` path. Send `init-chain.json` before transaction requests. Refresh query views and the explicitly trusted local signing context after a commit. Initial context files describe height zero only.

Remove the entire disposable directory after qualification. Retain only public results and hashes in evidence. These tests do not approve production parameters, custody, consensus deployment or a mainnet launch.
