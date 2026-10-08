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
  table, enabled units). The validator's signing state changes as it signs,
  so only its owner and mode are checked after the install.
- `aa-status | grep dytallix` and `nft list table inet dytallix_node`: the
  profiles and the firewall as loaded.
- On the sentry, `systemctl list-timers dytallix-backup.timer` and
  `journalctl -u dytallix-backup`: the off-host copies, one line per run
  (`UPLOADED` with the height and object name, or `NOTHING_NEW`);
  `/var/lib/dytallix-backup/last-uploaded` is the last height copied
  ([disaster recovery v1](../architecture/disaster-recovery-v1.md)).

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

The Host install workflow runs the same install, start, verify, restart and
wipe on an Ubuntu 24.04 runner for every change (`staging_host.py`).

## Moving to a new release

For an approved upgrade or handover (P01, 6 October 2026,
[approval](../../../launch/approvals/P01_E05_HOST_RECOVERY_2026-10-06.json)):
stage the new release beside the running one before activation, and switch
when the old release stops at the activation height.

1. **Before activation**, on every host, with the host's bundle for the new
   release (downloaded and checked like the install bundle):
   `./dytallix-host/stage.sh`. It installs the new release's binaries and
   configuration under its own `/opt/dytallix/NEW` and
   `/etc/dytallix/NEW`, and the unit, profiles, firewall and journal setting
   it will switch to under `/etc/dytallix/NEW/next`. The node home and keys
   are kept and nothing is unsealed; a bundle for another host or chain, or
   one that changes the node home, is refused. Releases older than the
   running one are removed.
2. **At activation** the old release stops itself: the committed release is
   no longer its own (exit class `release`).
3. On each host, from the new bundle: `./dytallix-host/switch.sh`. It
   refuses while the node runs. It installs the staged unit, profiles,
   firewall and journal setting, unloads the old release's profiles,
   verifies the host against the new bundle and starts the node. The
   previous release stays installed until the next stage.

## Validator recovery

[validator-recovery.md](validator-recovery.md) (F17): fence the old server
first, never restore signing state, set the old votes aside.

## Disaster recovery

[disaster-recovery.md](disaster-recovery.md) (F19): a lost sentry or
endpoint is installed again from its bundle and restored from the newest
off-host copy with `restore.sh`, then catches up from the validator. The
copy carries the light blocks the restore verifies, so nothing reaches the
console-only host but the copy.
