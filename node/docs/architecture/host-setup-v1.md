# Host setup v1

Engineering task E05. How a bare server becomes a running Dytallix node of
one role (validator, sentry or endpoint) under the solo launch profile:
three hosts, the founder alone, console access only
([solo launch](../../../launch/approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)).
This is the design; the tools follow in steps H2 to H5.

## Decisions (P01, 6 October 2026)

- **Operating system.** Ubuntu 24.04 LTS on every host. It is the only system
  the node's execution policy has run on (CI and the E02 diagnostics: kernel
  6.8, AppArmor 4.0.1, whose `abi/4.0` file the policy pins).
- **Addressing.** Each host's public IPv4 address is on its own network
  interface, one per node, with no NAT. The engine listens on, dials from
  and firewalls the same address (production activation v1, D12-Q01).
- **File movement.** Node keys are made offline on the ceremony machine. Each
  host gets one bundle: the release, its configuration, the chain's genesis
  and root signatures, and its node keys sealed under a passphrase
  (AES-256). The bundle is public. At the host console the founder types
  only the bundle's SHA-256 and the passphrase; nothing ever has to come off
  a host. The sealed keys are also their backup.
- **Layout.** The service account and paths below.

## Layout

| Path | Owner, mode | Holds |
| --- | --- | --- |
| `/opt/dytallix/RELEASE/bin/` | root, 0555, files 0555 and immutable | The release's static binaries (`RELEASE` is the first 16 hex digits of the release manifest's SHA-512) |
| `/etc/dytallix/RELEASE/` | root, 0755, files 0444 | The release manifest, catalog mapping, root, emergency verifier and candidate configurations, process admission, root policy and signatures, application configuration and native genesis, the service configuration |
| `/var/lib/dytallix/node/` | `dytallix`, 0700 | The node home: `config/` (engine files and keys, 0400 or 0600), `data/`, `abci/`, `appdb/` |
| `/var/lib/dytallix/lock/` | `dytallix`, 0700 | The supervisor's lock |
| `/var/lib/dytallix/metrics/` | `dytallix`, 0755 | Metrics text files |
| `/var/lib/dytallix/snapshots/` | `dytallix`, 0700 | State sync snapshots, where configured |
| `/var/lib/dytallix/light-blocks/` | root, 0755 | Operator light block exports, for a state sync join |
| `/var/lib/dytallix/scratch/` | `dytallix`, 0700 | The root helper's scratch directory |
| `/etc/systemd/system/dytallix-node.service` | root, 0644 | The unit |
| `/etc/apparmor.d/dytallix/` | root, 0644 | The four role profiles |
| `/etc/nftables.d/dytallix.nft` | root, 0644 | The host firewall table `inet dytallix_node` |

The service account is the system user and group `dytallix`, UID and GID
41001, with no login shell. Every path the unit writes is under
`/var/lib/dytallix` and is mounted noexec by the unit. Code exists only
under `/opt/dytallix`, which the unit mounts read-only.

## The pipeline

```text
offline (ceremony machine)          online (founder's machine)             each host (console, root)
node keys per host ──sealed──┐
pin plan, host values ───────┼─> generate host files ─> bundle ─> publish ─> download, check SHA-256,
release, genesis, root sigs ─┘                                               unseal, install, verify,
                                                                             start
```

1. **Plan.** The founder writes the pin plan (`PIN_PLAN.json`, the three
   hosts' roles, addresses, ports and pins;
   [host configuration](../mainnet/host-configuration.md)) from a template.
2. **Node keys, offline.** On the ceremony machine, for each host: the
   validator key and its fresh signing state (`dytallix-validator-key`), the
   peer seed (`dytallix-peer-seed`) and, for the endpoint, the client channel
   seed (`dytallix-channel-key`). The public keys go into the pin plan, the
   genesis (the validator's key) and the endpoint's published channel pin.
   The private files are sealed for their host.
3. **Generate, online.** `dytallix-host-config` writes each host's engine
   files and binding. A new generator then writes everything else for each
   host from the release, the genesis and these choices:
   - the catalog mapping (each release member's installed path);
   - the execution policy request and unit identities;
   - the role profiles, unit properties and process admission
     (`native-execution-policy/production_roles.py`);
   - the root, emergency verifier and candidate configurations;
   - the supervisor's service configuration, with the SHA-256 of every file
     it pins;
   - the systemd unit;
   - an install manifest: every file's destination, owner and mode.
4. **Bundle.** One archive per host: the release binaries, its files, the
   install manifest, `install.sh` and its sealed keys. Published with the
   release; its SHA-256 is noted on paper for the console.
5. **Install, at the host console.** As root:

   ```text
   curl -fLo bundle.tar URL
   echo "SHA256  bundle.tar" | sha256sum -c
   tar -xf bundle.tar && ./dytallix-host/install.sh
   ```

   `install.sh` refuses a host that is not Ubuntu 24.04 with AppArmor
   enabled. It creates the account and directories, unseals the keys (asking
   for the passphrase), installs every file as the manifest says, marks the
   binaries immutable, installs and loads the profiles in enforce mode,
   installs the firewall and enables nftables, installs and enables the
   unit, and then verifies: file hashes, owners and modes, profiles in
   enforce mode, the firewall table loaded. It never overwrites an existing
   node home.
6. **Start.** `systemctl start dytallix-node`. The supervisor runs its own
   startup checks before any child starts.

The TLS download is only transport: the typed SHA-256 is what the host
trusts. The bundle's release files are the ones whose manifest SHA-512 the
signed genesis binds.

## The unit

`dytallix-node.service` runs `/opt/dytallix/RELEASE/bin/dytallix-native-supervisor
--service-config /etc/dytallix/RELEASE/service.json` as `dytallix`, with the
rendered unit properties (`NoNewPrivileges`, the system call filter, an empty
capability set, `ProtectSystem=strict`, the read-only and no-exec paths, the
resource limits and the supervisor's AppArmor profile). It is
`Type=simple`, `Restart=no` (an operator restarts it after a refusal or a
halt; the runbooks rely on that), `TimeoutStopSec=60` (the supervisor's stop
and kill budget is 40 seconds), and it starts after `apparmor.service`,
`nftables.service`, `time-sync.target` and `network-online.target`. The
supervisor's report goes to the journal.

## Host packages and settings

- `apparmor`, `apparmor-utils` and `nftables` (`install.sh` checks them;
  installing them is part of the operator's base setup).
- `ufw` and `firewalld` disabled; the node's table is the host firewall.
- Time from `systemd-timesyncd`, Ubuntu's default, which `install.sh`
  checks is synchronized.
- The journal kept on disk for 90 days (`Storage=persistent`,
  `MaxRetentionSec=90d`), the operations objectives' log retention.
- Nothing else runs on the host.

## Values to approve

These have no approved value yet. The generator takes them from one host
values file; proposals come from the tested harnesses and are listed with
the generator for P01:

- **Unit resources** per role: memory, tasks and open files.
- **Ports:** P2P, the endpoint's client channel and its status page.
- **Catalog bounds** (manifest, mapping and member sizes and counts) and
  **observation bounds** (process maps, files and elapsed time).
- **The root helper's execution policy and root configuration bounds**
  (helper, request and genesis sizes, timeout).
- **The emergency verifier timeout**, still a value to measure.

## Staging, then wipe

The solo launch profile stages on the production hosts before genesis and
wipes them afterwards. The same bundle and `install.sh` install the staging
chain; a separate `wipe.sh` removes everything under `/opt/dytallix`,
`/etc/dytallix` and `/var/lib/dytallix`, the unit, profiles and firewall
table, so the production install starts clean.

## Steps

- **H1.** This design and the decisions.
- **H2.** The generator and the templates (pin plan, host values).
- **H3.** Sealing and the bundle; `install.sh` and `wipe.sh`.
- **H4.** A CI job that installs a staging bundle on an Ubuntu 24.04 runner
  and starts the node.
- **H5.** The runbooks that change with it: start and stop, a release switch
  on a host, validator recovery, rebuilding the endpoint by state sync.

## Open on staging

- Whether the engine writes `config/addrbook.json` under the read-only
  configuration (strict address book, PEX off).
- The engine, bridge and adapter write their logs to /dev/null today; only
  the supervisor's report and the application's errors reach the journal.
