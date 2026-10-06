# Host operations

Day-to-day operation of a host installed from its bundle
([host setup v1](../architecture/host-setup-v1.md)); the incident runbooks
are in the [index](README.md). Run every command as root at the host
console. `RELEASE` is the first 16 hex digits of the release manifest's
SHA-512 (the directory under `/opt/dytallix` and `/etc/dytallix`) and
`LABEL` is the host's label. Keep the unpacked bundle (`dytallix-host/`) on
the host: `verify` and `wipe.sh` read its install manifest.

## Status

- `systemctl status dytallix-node`: running, or how it last stopped.
- `journalctl -u dytallix-node -o cat`: the supervisor's report, one JSON
  object per line; a stop ends with `failure`
  ([where to look](README.md#where-to-look)).
- `/var/lib/dytallix/metrics/dytallix-app.prom` and `dytallix-engine.prom`:
  `dytallix_app_height` is the committed height, and a stale
  `dytallix_metrics_written_timestamp_seconds` means a process stopped
  writing.
- `python3 -I -B dytallix-host/host_install.py verify`: `PASS`, or every
  difference between the host and its bundle (file contents, owners and
  modes, immutable binaries, AppArmor profiles in enforce mode, the firewall
  table, enabled units).
- `aa-status | grep dytallix` and `nft list table inet dytallix_node`: the
  profiles and the firewall as loaded.

## Start and stop

- **Start:** `systemctl start dytallix-node`. The supervisor runs its
  startup checks before any child starts; a refusal stops the unit with a
  final report naming the failure (see the
  [exit classes](README.md#application-exit-classes)).
- **Stop:** `systemctl stop dytallix-node`. The supervisor stops its
  children within 40 seconds; the unit allows 60.
- The unit never restarts itself (`Restart=no`). After a refusal or a halt,
  read the report, follow its runbook, then start it again.
- The unit is enabled, so the node starts at boot, after AppArmor, nftables
  and time synchronization. After a reboot, run `verify` and check that the
  height advances.
- Never run the node's binaries by hand outside the unit. The unit applies
  the system call filter, the AppArmor profile and the limits that the
  supervisor's startup checks expect.

## What never changes on a host

- Files under `/opt/dytallix/RELEASE` and `/etc/dytallix/RELEASE`. The
  binaries are immutable and the supervisor pins every file's SHA-256, so an
  edited file stops the node at its next start. A change is a new bundle.
- Where the node keys live. Never copy the node home or its keys to another
  host, and never install a validator's bundle on a second host while the
  first could still run: one key signing in two places is a double sign,
  and the first double sign removes the validator for good
  ([penalties v1](../architecture/penalties-v1.md)).

## Staging: wipe and reinstall

The production hosts are the staging environment until genesis
([host setup v1](../architecture/host-setup-v1.md#staging-then-wipe)).

1. `systemctl stop dytallix-node`.
2. `./dytallix-host/wipe.sh` and type `wipe LABEL`. It removes the node, its
   keys and chain data, the unit, the profiles, the firewall table and the
   journal setting, and keeps the account.
3. Install the next bundle as in host setup v1, step 5.

The Host install workflow runs the same install, start, verify and wipe on
an Ubuntu 24.04 runner for every change (`staging_host.py`).

## Not yet written

- Moving a host to a new release for an approved upgrade or handover.
  `install.sh` refuses a host that already has an install.
- Validator recovery (F17) and disaster recovery (F19). The
  [operations objectives](../../../launch/operations/OBJECTIVES.md) fix their
  rules: a validator's signing state is never restored from a backup, and a
  replacement signs only after the old host is fenced off.
- Rebuilding the endpoint by state sync.
