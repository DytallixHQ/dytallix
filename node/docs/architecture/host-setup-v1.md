# Host setup v1

Engineering task E05. How a bare server becomes a running Dytallix node of
one role (validator, sentry or endpoint) under the solo launch profile:
three hosts, the founder alone, console access only
([solo launch](../../../launch/approvals/P01_E05_SOLO_LAUNCH_2026-10-03.json)).
This is the design. Steps H2 (the generator), H3 (node keys, bundles and
the installer) and H4 (the CI install) are built; H5 follows.

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
- **Sealing** ([sealing approval](../../../launch/approvals/P01_E05_HOST_SEALING_2026-10-06.json)).
  The passphrase is a seal code: 32 random bytes that `dytallix-root-sign
  seal` makes and prints once as a checked paper line, one code per host.
  The founder writes each code twice and keeps the two copies in two
  separate places of their own, never with a holder's kit (the kits have no
  paper and are held by five people since
  [7 October 2026](../../../launch/approvals/P01_E05_ROOT_KEY_HOLDERS_2026-10-07.json)). The key is SHAKE256 of a domain, the host's label
  and the code; the cipher is AES-256-GCM, authenticating the label and each
  file's path, size and SHA-256. A 256-bit code matches the kits' secrets
  (NIST category 5); the bundles are public and permanent, so the code must
  resist offline guessing indefinitely.
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
| `/var/lib/dytallix/snapshot-light-blocks/` | `dytallix`, 0700 | The engine's light blocks for each snapshot, beside the snapshots |
| `/var/lib/dytallix/restore/` | root, 0755 | An opened off-host copy during a one-time restore (`restore.sh`) |
| `/var/lib/dytallix/light-blocks/` | root, 0755 | Operator light block exports, for a state sync join |
| `/var/lib/dytallix/scratch/` | `dytallix`, 0700 | The root helper's scratch directory |
| `/etc/systemd/system/dytallix-node.service` | root, 0644 | The unit |
| `/etc/apparmor.d/dytallix-node` | root, 0644 | The four role profiles, in the file `apparmor.service` loads at boot |
| `/etc/nftables.d/dytallix.nft` | root, 0644 | The host firewall table `inet dytallix_node` |
| `/etc/dytallix-backup/` | root, 0700, files 0400 | The sentry's backup code and upload key ([disaster recovery v1](disaster-recovery-v1.md)) |
| `/var/lib/dytallix-backup/` | root, 0700 | The backup job's last uploaded height and scratch copy |
| `/etc/systemd/system/dytallix-backup.{service,timer}` | root, 0444 | The sentry's hourly off-host backup |
| `/etc/dytallix-monitor/` | root, 0700, files 0400 | The host's monitor settings, `webhooks.json`, installed by `monitor-settings.sh` and never in the bundle; the history code and upload key, `history-code` and `history-upload.json`, unsealed with the node keys ([monitoring v1](monitoring-v1.md)) |
| `/var/lib/dytallix-monitor/` | root, 0700 | The monitor job's state and metric history |
| `/etc/systemd/system/dytallix-monitor.{service,timer}` | root, 0444 | The monitor job, every minute, on every host |

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
   [host configuration](../mainnet/host-configuration.md)) from
   [`launch/hosts/PIN_PLAN.template.json`](../../../launch/hosts/PIN_PLAN.template.json):
   the validator pins its sentry, the sentry pins the validator and the
   endpoint, and the endpoint pins the sentry.
2. **Node keys, offline.** On the ceremony machine
   ([key ceremony, node keys](../../../launch/custody/KEY_CEREMONY.md#node-keys)),
   `host_keys.py` makes, for each host in the plan: the validator key and its
   fresh signing state (`dytallix-validator-key`), the peer seed
   (`dytallix-peer-seed`) and, for the endpoint, the client channel seed and
   its pin (`dytallix-channel-key`). It writes the key summaries, seals each
   host's private files (`dytallix-root-sign seal`) and has the founder type
   each printed seal code back from paper (`seal-check`). Its output is
   public: the summaries, the sealed records, the endpoint's channel pin and
   the pin plan with the public keys filled in, which also go into the
   genesis (the validator's key).
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
4. **Bundle.** `host_bundle.py` writes one deterministic archive per host:
   the release binaries, its files, the install manifest, `install.sh`,
   `wipe.sh`, `host_install.py` and its sealed keys, all checked against the
   manifest first. Published with the release; its SHA-256 (also written
   beside it) is noted on paper for the console.

   ```text
   host_bundle.py --host-files OUT/LABEL --release RELEASE_DIR \
     --sealed LABEL.sealed.json --out LABEL.bundle.tar
   ```
5. **Install, at the host console.** As root:

   ```text
   curl -fLo LABEL.bundle.tar URL
   echo "SHA256  LABEL.bundle.tar" | sha256sum -c
   tar -xf LABEL.bundle.tar && ./dytallix-host/install.sh
   ```

   `install.sh` runs `host_install.py install`. It refuses a host that is
   not Ubuntu 24.04 with AppArmor enabled, the tools present, the clock
   synchronized and `ufw` and `firewalld` inactive, and any host that
   already has an install. It checks every bundled file against the
   manifest, creates the account and directories, installs the binaries and
   every file as the manifest says, asks for the seal code and unseals the
   keys with the release's `dytallix-root-sign unseal` (which never replaces
   a file), sets owners and modes, marks the binaries immutable, loads the
   profiles in enforce mode, adds `include "/etc/nftables.d/*.nft"` to
   `/etc/nftables.conf` and enables nftables, applies the journal setting,
   enables the unit, and then verifies: file hashes, owners and modes, the
   immutable flag, profiles in enforce mode, the firewall table loaded and
   both units enabled. `host_install.py verify` repeats the check later.
6. **Start.** `systemctl start dytallix-node`. The supervisor runs its own
   startup checks before any child starts.

The TLS download is only transport: the typed SHA-256 is what the host
trusts. The bundle's release files are the ones whose manifest SHA-512 the
signed genesis binds.

## Key summaries

The offline key step writes, for each host, `LABEL.keys.json`
(`dytallix.host-keys.v1`): the label, role, peer and validator public keys,
and `secret_files`, the SHA-256 of each private file the sealed keys hold
(`config/pqc_peer_seed.bin`, `config/priv_validator_key.json`,
`data/priv_validator_state.json` and, on the endpoint,
`config/client_channel_seed.bin`), and for the endpoint its public
`LABEL.channel-pin.json`. The generator pins the digests; it never sees a
private file.

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

- `apparmor`, `apparmor-utils`, `nftables` and `python3` (`install.sh`
  checks them; installing them is part of the operator's base setup).
- `ufw` and `firewalld` disabled; the node's table is the host firewall.
- Time from `systemd-timesyncd`, Ubuntu's default, which `install.sh`
  checks is synchronized.
- The journal kept on disk for 90 days (`Storage=persistent`,
  `MaxRetentionSec=90d`, in `/etc/systemd/journald.conf.d/dytallix.conf`,
  which the generator writes), the operations objectives' log retention.
- Nothing else runs on the host.

## Host values (P01, 6 October 2026)

[`launch/hosts/SETUP_VALUES.json`](../../../launch/hosts/SETUP_VALUES.json)
holds the generator's approved values beyond the engine settings
([host values approval](../../../launch/approvals/P01_E05_HOST_VALUES_2026-10-06.json)),
proposed from the node's tested harnesses:

- **Unit limits,** every host: memory 6 GiB, so 8 GB hosts suffice; 4,096
  tasks; 65,536 open files.
- **Ports:** P2P 26656; the endpoint's client channel 26670 and status page
  8080.
- **Code owners:** root only.
- **Catalog checks:** manifest and mapping 1 MiB, 32 members, 16 roles and
  runtime profiles, 128 references, 128-byte IDs, 4,096-byte paths, 128 MiB
  per file and 512 MiB in all.
- **Process observation:** 64 KiB stat, 2 MiB maps, 4,096 map entries,
  4,096-byte paths, 64 files, 128 MiB per file, 512 MiB in all, 15 seconds.
- **Root configuration:** a 16 MiB helper, 4 MiB requests (the rehearsal's
  genesis bundle is about 0.2 MB encoded), a 9 MiB engine genesis, a 1 MiB
  release manifest, a 30-second timeout; the helper's own observation
  bounds.
- **Emergency verifier:** a 30-second timeout.

The staging hosts re-check the memory cap and the timeouts before genesis.

## Staging, then wipe

The solo launch profile stages on the production hosts before genesis and
wipes them afterwards. The same bundle and `install.sh` install the staging
chain; a separate `wipe.sh` removes everything under `/opt/dytallix`,
`/etc/dytallix` and `/var/lib/dytallix`, the unit, profiles, firewall table
and journal setting, so the production install starts clean. It asks the
founder to type `wipe LABEL` first and keeps the account.

## Steps

- **H1.** This design and the decisions.
- **H2.** The generator and the templates (pin plan, host values).
- **H3.** Sealing and the bundle; `install.sh` and `wipe.sh`. Built:
  `dytallix-root-sign seal`, `seal-check` and `unseal`; `host_keys.py`,
  `host_bundle.py` and `host/` (`host_install.py`, `install.sh`,
  `wipe.sh`), tested on a synthetic network with a stand-in system
  (`test_host_bundle.py`).
- **H4.** A CI job that installs a staging bundle on an Ubuntu 24.04 runner
  and starts the node. Built: the Host install workflow
  (`.github/workflows/host-install.yml`) builds the release set in the
  pinned builder; `staging_host.py prepare` runs the key step with the
  release's tools and `staging_chain.py` builds a throwaway
  production-profile staging chain (`dytallix-staging-1`: the production
  rehearsal's invented records with this run's validator key, the release
  manifest bound in its freeze order, five throwaway root keys signing three
  of five, host configuration, host files and bundles); `staging_host.py
  install` runs the validator's `install.sh` with its seal code, starts the
  unit, waits for the application's height metric, verifies the host and
  `wipe` removes it. The same two commands rehearse a staging host by hand.
  The workflow's second job runs all three roles (`staging_network.py`):
  the validator on the runner and the sentry and the endpoint in two KVM
  virtual machines on a private bridge, each with its own address on its
  own interface and its own bundle, driven through the QEMU guest agent
  (their firewalls drop everything else). The sentry syncs from the
  validator, the endpoint through the sentry, the endpoint's status page
  answers, the sentry snapshots (a staging interval of 10 blocks) and its
  backup job uploads an encrypted copy to a stand-in store
  (`s3_standin.py`), which the runner opens with the backup code and
  compares with the snapshot.
- **H5.** The runbooks that change with it: start and stop, a release switch
  on a host, validator recovery, rebuilding the endpoint by state sync.
  Built: [host operations](../operations/host.md) (status, start and stop,
  staging wipe, and the release switch: `stage.sh` and `switch.sh` over
  `host_install.py stage|switch`, P01, 6 October 2026) and
  [validator recovery](../operations/validator-recovery.md) (F17). Open:
  disaster recovery (F19) and the endpoint rebuild.

## Open on staging

- Whether the engine writes `config/addrbook.json` under the read-only
  configuration (strict address book, PEX off).
- Role logs (resolved): the engine, bridge and adapter write their standard
  output and error to a private pipe each; the supervisor copies each line
  to the journal prefixed with the role (`consensus_engine: ...`), cut at
  4,096 bytes, at most 1,000 lines per role in 30 seconds with the rest
  counted (`[N lines dropped ...]`), so a role cannot crowd out the
  supervisor's report or reach the journal's rate limit. The H4 install
  requires the engine's lines in the validator's journal. The engine stays at
  info level; its log meets the log policy (P01, 10 October 2026,
  [approval](../../../launch/approvals/P01_E05_ROLE_LOGS_2026-10-10.json)).
