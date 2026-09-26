# Restricted service account

The service runs as `dytallix`. It requires an explicit environment file and
checks runtime paths before startup. It does not provision or replace keys.
The unit sets `DYT_REQUIRE_EXISTING_VALIDATOR_KEY=1`. Do not override that value.
In this mode, the plain-key provider performs no writes. The default FIPS 204
backend rejects an unsupported existing key, and startup terminates on failure.

## Migrate an existing testnet service

1. Record the current chain ID, validator ID, public proposer address, binary
   digest, database path, genesis digest, and key-provider configuration.
2. Stop the existing service. Confirm that no second process uses the database.
3. Back up the database, genesis, configuration, and existing key material.
   Check the backup before changing ownership or paths.
4. Create a dedicated system account and group named `dytallix`. Keep the
   executable, source tree, genesis, scripts, and configuration root-owned.
5. Place the existing database and plain key directory outside `/root` and
   other home directories. Use dedicated paths such as
   `/var/lib/dytallix/data` and `/var/lib/dytallix/keystore`.
6. Give `dytallix` access only to the required data and key paths. Restrict
   plain key files to mode 0600 and key directories to mode 0700. The legacy
   `.seal` filename does not establish that the file is encrypted.
7. Set `DYT_DATA_DIR`, `DYT_KEYSTORE_DIR`, and the existing `VALIDATOR_ID`
   explicitly in `/etc/dytallix/dytallix-fast-node.env`. Use absolute paths.
   For Vault, preserve the URL, token, mount, base path, and validator ID.
   Do not copy secrets into logs or reports.
8. Keep `/opt/dytallix-node` as the working directory. Startup reads
   `./genesis.json`. Preserve the intended relative configuration files.
9. Run `check-runtime-paths.sh` under the service identity with the intended
   environment. The check must pass before installation of the new service.
10. Install the unit and start one node. Verify the same chain and proposer
    identity. Test signing, stop/start continuity, and backup restoration.

The preflight requires an existing readable, nonempty plain key. It rejects
an incomplete Vault configuration. Vault key validity still requires the
node's provider check and the signing test. A path check is not a key audit.

## New deployments

Provision and verify the intended validator key before using this service.
Do not rely on the development key-generation fallback. Set the explicit data
directory, validator ID, and provider configuration. Populate genesis through
the approved deployment procedure.

`ProtectHome=true` deliberately prevents access to keys under user home
directories. `ProtectSystem=full` protects system directories. File ownership
must also prevent the service from modifying its executable or scripts.

## Verification limits

The unit must pass Linux `systemd-analyze verify` and actual service tests.
Confirm required outbound access to peers and Vault under the host network
policy. A local macOS script check does not qualify this Linux service.
Do not describe this testnet service migration as mainnet approval.
