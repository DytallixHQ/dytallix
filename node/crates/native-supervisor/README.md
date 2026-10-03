# Native supervisor

This package implements the native owner of a Linux node service. Each build has one mode:

- a development build runs only `disposable-loopback-native`, a disposable loopback service that refuses production chain names;
- a production build (cargo feature `production`) runs only `production-native`, described under [Production mode](#production-mode).

Neither mode authorizes a launch. A production chain starts only from a root genesis signed three of five (production activation v1).

The service performs these steps:

1. Check explicit configuration pins and the private state paths.
2. Acquire the existing home and validator-signing-identity locks. Preserve both lock files.
3. Verify root authorization and committed release history with the independently pinned root helper and read-only storage. Require its explicit private scratch directory before running that helper.
4. Select the candidate digest from verified authority. Verify the complete V2 catalog and configured helper role bindings.
5. Observe the supervisor's actual executable and mapped files.
6. Start and retain the application child. Check its information response against the preflight state. Observe its executable and mapped files.
7. Transfer application pipes to the bridge on descriptors 3 and 4. Observe the bridge.
8. Start and observe the engine. Verify its private RPC peer identity, chain and application state. Start the optional adapter only when the catalog declares it. Verify its owned listener and chain response.
9. Check child health, retained locks and process mappings at the explicit interval. Stop owned children on failure or cancellation.

Child descriptors 5 and 6 retain the lifecycle locks. The bridge inherits descriptors 3 and 4 in addition to these locks. The owner clears child environment variables, then applies the explicit allowlist. No process is adopted from a PID.

Run a development build with `--development-service-config` and a production build with `--service-config`, each with one absolute configuration path. `config::NativeServiceConfig` defines the strict input schema. All hash pins, paths, resource limits and listener ports are required inputs. This package contains no production configuration defaults.

The configuration also sets the node's outputs (E04 gap 15):

- `metrics` (required): `directory` and `interval_seconds` (1 to 3600). The engine writes `dytallix-engine.prom` and the application `dytallix-app.prom` there (metrics v1). The directory is owned by the service user, not group or other writable, and outside `config`, `data`, `abci` and `appdb`, so an operator agent can read it.
- `snapshots` (optional): `directory`, `interval_blocks` and `keep`. The application writes state sync snapshots there and the bridge serves them. The directory is mode 0700 and outside the same protected paths.
- `block_history` (required): `window` keeps the retained window of block records; `archive` keeps every record.
- `restart_authorization` (optional, E04 gap 18): a pinned restart authorization, at most 256 KiB. After a halt, the preflight verifies it against the committed checkpoint and selects its target release, and the application runs the halted block on that release. Once its receipt is committed, the same file is ignored; remove the pin afterwards.

Each directory must lie inside one of the unit's writable roots. The values are E05 inputs; nothing has a default.

The configuration pins the application configuration, application genesis, root configuration, root request and policy, emergency verifier configuration, V2 candidate configuration and five engine inputs. The V2 catalog separately binds executable and library bytes. An input pin proves byte identity; it does not approve a release.

## Production mode

A production build runs the mode `production-native` (production activation v1, step A5; design [production activation v1](../../docs/architecture/production-activation-v1.md)). The steps above apply, with these differences.

- **Root.** The preflight verifies the threshold root genesis (three of five signatures) with `preflight_release_with_root`. `root_public_inputs` pins the signer policy and the combined signatures file named by the root configuration. The application starts with `--root-config`, `--verifier-config` and `--candidate-config`; no development flag exists in the build.
- **Catalog.** The release catalog has the service profile `production-linux-native-service-v1`. One catalog serves every role, so it always carries the HTTP adapter.
- **Role** (required): `validator`, `sentry` or `endpoint`.
  - Validators and sentries run no adapter and no channel listener.
  - An endpoint runs the HTTP adapter on loopback and its client channel on the node's address. It may also serve the read-only status page for an uptime checker (`adapter_status_listen`, P01, 3 October 2026) on the node's address, on a port of its own.
  - The supervisor passes the adapter its own build's profile: `dytallix-pqc-http-production-v1` in a production build.
  - A sentry or endpoint key may not be in the genesis validator set, and the engine gives a sentry or endpoint a signer that refuses every signature. A validator's key may be outside the set: a validator registered after genesis starts before its key is active (P01, 3 October 2026).
- **Identity.** The engine inputs pin `config/pqc_peer_seed.bin` in place of `node_key.json`, which must be absent. Create the seed on the host with `dytallix-peer-seed generate --home HOME`; it prints the public key for the node's transport file and its peers' pins.
- **Binding** (required): `binding` pins the host's published binding (at most 4 KiB). Before any child starts, the supervisor checks it against the pinned configuration, genesis and transport files, the transport's peer key, the validator key and the role. The engine checks it again with `--binding`. Print a host's binding with `dytallix-peer-seed binding --home HOME --role ROLE`; it must equal the binding `dytallix-host-config` generated for the host from the published pin plan ([host configuration](../../docs/mainnet/host-configuration.md)).
- **Network.** The engine's P2P listener is the node's one address: a canonical global unicast IP and a port from 1024 (P01, 30 September 2026). An endpoint's channel listener uses the same IP on another port. The engine runs the production transport profile `dytallix-pqc-production-v1`.
- **State sync** (optional): `state_sync.light_blocks` lists one to 16 operator light block exports, the first the primary. They are required exactly when the engine configuration enables state sync. Each is an absolute directory outside the home, owned by root or the service user, without group or other write. The supervisor passes each with `--light-blocks`.
- **Readiness.** `catch_up_millis` (required; from `process.startup_millis` to seven days) is how long readiness waits for the engine to catch up (P01, 1 October 2026). While waiting, the supervisor rechecks its children, both locks and its own security state at every poll. Each round of probes keeps the startup bound.
- **Monitoring.** Each long-running child (engine, application, bridge and adapter) is observed once, paused, at startup. Each later tick checks liveness, the locks and each child's security state (UID, GID, AppArmor label, no-new-privileges, seccomp and mount namespace) without pausing it (P01, 30 September and 1 October 2026). The application's helpers keep their paused checks.
- **Reports** use the scopes `PRODUCTION_NATIVE_SERVICE_SNAPSHOTS` and `PRODUCTION_NATIVE_SERVICE_FINAL_TIMINGS` and name the mode and role.

The values (`catch_up_millis`, process limits, outputs) are E05 operating values with no defaults. Render the unit, AppArmor profiles and host firewall rules with `tools/native-execution-policy` (routed mode). The rules admit only the pinned peers to the P2P listener.

## Qualification limits

Current-process and child observations are snapshots. Periodic observation cannot prevent a mapping that appears and disappears between checks. Unknown file mappings are refused; no general data-file mapping exception exists. In production mode, long-running children are observed only at startup; the kernel limits (AppArmor execution rules, MemoryDenyWriteExecute, seccomp and no-new-privileges) carry that control afterwards, and native qualification (T03) must show they hold.

The application still owns the short-lived root and control helper processes. End-to-end helper observation and custody qualification remain separate requirements. An inherited parent-death signal on direct children does not prove cleanup of every possible descendant.

The locks exclude cooperating owners that use the same protected lock directory. They do not prevent an unrelated process from bypassing the supervisor. Protected deployment configuration and service containment remain required.

The readiness probes establish process identity and protocol responses. They do not independently verify consensus proofs. Consensus commitment, restart, handover, final artifact inspection and production acceptance require separate evidence from the integrated candidate.

The `qualification-fixtures` feature adds synthetic process fixtures and Linux tests. It must remain disabled in release artifacts. These fixtures do not establish integrated service qualification.
