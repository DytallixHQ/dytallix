# Disposable PQC engine services

This directory supervises the actual consensus engine, ABCI bridge, and Rust application. Both manifest versions require `dytallix-pqc-loopback-v1` peer transport. Version 2 adds private Unix RPC, the Hyper loopback adapter, and development root authorization. Production remains NO GO. The service does not generate keys, reset signing state, remove databases, or approve artifacts.

## Process ownership

One systemd instance owns one Python supervisor. The supervisor starts the bridge first. The bridge starts the Rust application. After the private ABCI socket exists, the supervisor starts the engine. The engine then performs its own configuration, key, and peer checks.

The supervisor holds two advisory locks: one for the home directory and one for the full validator public identity. Every systemd instance uses `/run/dytallix-pqc`. A second cooperating instance cannot use the same home or validator identity. Child engine and bridge processes inherit the lock descriptors. The supervisor never deletes lock files.

These locks do not prevent an administrator from running an unmodified executable outside this service. They do not detect a copied signing key on another host. Custody controls and operator coordination remain required.

The supervisor stops the engine before the bridge. It stops both process groups if either child exits. The systemd unit uses `KillMode=control-group` so systemd also controls remaining descendants. Automatic restart is disabled. An operator must resolve a failed startup or stale socket before another start.

## Prepare disposable Linux staging

This procedure requires a separately approved disposable host. These files do not provision a host or modify a firewall.

1. Install Python 3.11 or later. Supply the reviewed Linux engine, bridge, and `consensus_stdio` executables. Keep the installation under `/opt/dytallix-node`. Make the installation root-owned and unwritable by the service identity.
2. Create the dedicated non-root user and group `dytallix-pqc`. Do not give this account an interactive login or administrative access.
3. Choose a lowercase instance name with at most 32 letters, digits, or hyphens. Start the name with a letter or digit.
4. Supply the existing initialized home at `/var/lib/dytallix-pqc/INSTANCE`. Supply `config`, `data`, `data/cs.wal`, `appdb`, and `abci` as mode-0700 directories owned by the service identity. Do not replace existing data or signing state. A fresh fixture can have empty `appdb` and `data/cs.wal` directories.
5. Supply the existing private node key, private validator key, validator signing state, engine genesis, and complete peer configuration. Use service-owned mode-0600 files. Keep `config` mounted read-only through the unit. The engine requires private file modes and validates the cryptographic keys itself.
6. Place the native genesis and application configuration in the instance `config` directory. Keep both files private. Set `config/config.toml` to the exact local key, database, socket, and WAL paths. Bind P2P and RPC only to numeric `127.0.0.1` addresses. Disable extra listeners and discovery.
7. Copy `manifest.template.json` to `/etc/dytallix-pqc/INSTANCE.json`. Replace every null field with the reviewed absolute path or SHA256. Use the SHA256 of the decoded 1952-byte ML-DSA-65 validator public key for `validator_public_key_sha256`. Do not put private key bytes in the manifest. Set the manifest owner to the service identity and its mode to 0600. Keep the containing directory root-owned. The unit mounts the manifest directory read-only.
8. Review the executable and configuration hashes against the evidence package. A locally calculated hash does not establish review or release approval.
9. Verify the unit on the target Linux host with `systemd-analyze verify`. Run the supervisor `check` as the service identity with `--manifest` and `--systemd-instance`. The systemd unit creates the shared runtime lock directory. For a pre-installation check, an administrator must first create that directory with the same service ownership and mode 0700.
10. Install the template only on the approved disposable host. Start one instance. Check process state, logs, exact chain identity, peer identity, and block progress. Start the remaining fixture instances. Stop and restart the instances. Verify state continuity before expanding the test.

The installed command has this form:

```text
/usr/bin/python3 -B /opt/dytallix-node/deploy/pqc-engine/supervise.py run --manifest /etc/dytallix-pqc/INSTANCE.json --systemd-instance INSTANCE
```

The unit enforces protected installation and configuration paths. It permits writes only to the instance data, application database, ABCI directory, and shared lock directory. It removes capabilities and limits address families. The 30-second manifest timeouts and journal rate limit are fixture settings. They are not approved production recovery or retention policies.

## Failure handling

If the ABCI socket exists before startup, the supervisor refuses to start. It does not remove the socket automatically. Stop the unit. Verify that no engine, bridge, or application still owns the home or signing identity. Check the socket owner and path. An authorized operator can then remove the confirmed inactive socket. Do not delete a database, WAL, key, lock file, or signing-state file during this procedure.

A backup must preserve database, WAL, configuration, keys, and signing state consistently. Do not restore old signing state after the validator has signed later blocks. This service provides no cross-version migration or rollback authority.

## Verification limits

The local test suite uses disposable fake child programs to check startup ordering, shutdown ordering, refusal conditions, and exclusive locks. These tests do not establish consensus correctness or key validity. Run the actual engine and application qualification separately.

Linux container tests and unit syntax verification do not establish a running systemd service. Final qualification requires an actual Linux service, the frozen candidate, the approved host inventory, monitoring, off-host backups, and operator evidence. G35 remains blocked until the exact production executables, loaded providers, interfaces and trust paths meet its acceptance requirements. A selected executable check does not qualify the full distribution. The service cannot grant gate acceptance.

## Combined development profile

Use `manifest-ipc-root.template.json` for manifest version 2. Version 1 retains its existing process behavior. Both versions refuse production mode.

Version 2 pins the Hyper adapter and all root inputs. Supply the root helper, root configuration, public policy, signed public request, actual engine genesis, and release manifest. The root configuration must name the same files and helper hash. Supply explicit external artifact byte bounds no larger than 4 MiB. The supervisor checks the external SHA512 digests. The application verifies the signed bundle and root state.

Set `ipc.listen` to the existing numeric loopback RPC host and port without the `tcp://` prefix. The engine runs with `--rpc-profile dytallix-pqc-unix-v1`. The supervisor waits for the private engine RPC socket before it starts Hyper. It passes `--development-root-config` to the application. It stops Hyper, then the engine, then the bridge. A child exit stops all owned groups.

For systemd, install the adapter and root helper under `/opt/dytallix-node`. Keep all root data inputs under the instance `config` directory. Use the existing protected unit and private service identity. The supervisor refuses an existing ABCI or RPC socket. After a failed run, an authorized operator must confirm that all processes have stopped before removing only a stale socket.

The combined native harness and service control tests are development evidence. They do not qualify production root custody, Linux x86_64, public HTTP interfaces, or hosted wallets.

The supervisor uses CPython's `_sha2` or `_sha256` and `_sha512` modules for artifact hashes. It has no `hashlib` or OpenSSL hash fallback. Startup fails if these built-in hash implementations are unavailable. Runtime events report the selected hash modules. This limits the supervisor hash path. It does not establish that the complete Python installation or operating system contains no classical cryptography.

## Optional development emergency verifier

Use `manifest-ipc-root-emergency.template.json` to add the optional `emergency` object to version 2. Use `emergency-verifier.template.json` for its local verifier settings. Omit `emergency` to retain the existing version 2 behavior. Explicit null, unknown fields, and version 1 emergency settings are refused.

Pin the exact verifier configuration and helper executable with SHA256. The configuration must contain exactly the six fields in its template. Its helper path and hash must match the manifest. The supervisor checks these files before child startup and again after it acquires both locks. The application independently checks the helper hash before each verification. The configuration file is limited to 64 KiB. Supply explicit positive integer limits: at most 256 MiB for helper bytes, 4 MiB for a helper request, and 60000 milliseconds for a helper call. These are local service limits. They are not control activation windows or production consensus rules.

Create a separate service-owned mode-0700 directory below the instance `abci` directory, such as `abci/emergency-helper`. Set `helper_scratch_path` to its absolute path. The supervisor requires an existing private directory with write and traversal access. It rejects symlinks. The mounted filesystem must permit execution because the verifier executes a hash-checked helper copy there. Local path and permission checks do not prove mount execution support; verify an actual helper call on the staging host. The existing unit permits writes under `abci`, so the unit needs no additional writable path.

The supervisor passes both `--development-root-config` and `--development-emergency-verifier-config` to `consensus_stdio` through the bridge. For systemd, keep the emergency configuration under the protected instance `config` directory and the helper under `/opt/dytallix-node`. Supply the emergency policy in the root-authorized application configuration. The application validates the signed policy and release binding. The service does not select authority keys, thresholds, activation timing or automatic transition policy.

Local service tests check exact argument propagation, omission, clean restart and rejection before child startup. Fake child programs do not verify an emergency signature, freeze transactions or prove block progress. Repeat those checks with the current compiled application, real helper and consensus engine on the assigned native staging host. Production remains NO GO.
