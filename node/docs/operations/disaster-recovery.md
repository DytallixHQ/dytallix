# Disaster recovery (F19)

The sentry, or an endpoint if one is added, is lost: destroyed,
unreachable for good, or its disk failed. It is rebuilt from its bundle and
restored from the newest off-host copy, then it catches up from the
validator
([disaster recovery v1](../architecture/disaster-recovery-v1.md#restore)).
The approved recovery time is 4 hours
([operations objectives](../../../launch/operations/OBJECTIVES.md)). With
two seats the sentry is also the public node
([two seats](../../../launch/approvals/P01_E05_TWO_SEATS_2026-10-11.json)): while it is down the validator keeps the chain running,
but no client can reach it.

| Lost | Procedure |
| --- | --- |
| The sentry, the validator alive | This page |
| An endpoint, if one is added | This page |
| The validator only | [Validator recovery](validator-recovery.md) (F17): the sentry holds every block |
| The validator and the sentry together | An incident decision first ([incident response](README.md)), then this page for both, the validator with `--accept-history-loss` |

## Rules

- **Fence first.** Power the old server off and delete it, or reinstall
  it, in the provider console, and see it gone before the replacement
  starts. Two hosts with one peer key and one address confuse every peer
  that pins them.
- **Restore only into a fresh install.** `restore.sh` refuses a host whose
  node has run; wipe it and install it again.
- **The copy must be at most 13 days old.** A restore trusts the copy's
  header for the evidence age less one day (P01, 8 October 2026,
  [approval](../../../launch/approvals/P01_E05_RESTORE_2026-10-08.json)),
  and the validator keeps blocks for at least 14 days. The sentry copies
  daily, so the newest copy is at most about a day old: restore a lost
  sentry within about 12 days. A copy older than 13 days does not verify.
- **The validator only after the incident decision.** Restoring the
  validator from a copy loses every block after it, and the chain resumes
  from the copy. `restore.sh` refuses on the validator unless
  `--accept-history-loss` is given; give it only once the incident decision
  is recorded.
- **If in doubt, wait.** The validator keeps the chain running while the
  sentry is down.

## You need

- Provider console access, and the storage provider's console for the
  copies.
- The host's bundle for the release the chain runs, and its SHA-256.
- One of the two paper copies of the host's seal code.
- One of the two paper copies of the chain's backup code
  (`dytallix-backup-CHAIN` and seventeen groups). Without either, no copy
  can be opened.

## Steps

1. **Declare** the incident and note the time.
2. **Confirm** the host is lost: the provider console shows it down or
   gone. For the sentry, its status page no longer answers, while the
   validator's height keeps rising
   (`grep '^dytallix_app_height' /var/lib/dytallix/metrics/dytallix-app.prom`
   at the validator's console, twice, a minute apart).
3. **Fence.** Delete the old server, or reinstall its operating system, in
   the provider console, and wait until it shows so.
4. **Provision** the replacement: Ubuntu 24.04 with the host base setup
   ([host setup v1](../architecture/host-setup-v1.md#host-packages-and-settings))
   and the old server's public IPv4 address, reassigned in the provider
   console. The pin plan and the peers' pins and firewalls name that
   address. If it cannot be reused, stop: a new address needs a new pin
   plan and new bundles for every host that pins it, which this procedure
   does not cover.
5. **Install** at the replacement's console as in host setup v1: download
   the bundle, check its SHA-256, unpack it, run
   `./dytallix-host/install.sh` and type the seal code. Do not start it.
6. **Fetch the newest copy.** In the storage console, find the newest
   complete copy under the chain's prefix: a folder
   `PREFIX/CHAIN/{height:020}-{sha256}/` (the height is in the name) that
   holds `manifest.json`. A folder without a manifest is an incomplete
   upload; skip it. Make a short-lived download link for the manifest and
   for each part it lists (`part-0000.bin` onwards, 1 GiB each). On the
   replacement:

   ```text
   mkdir /root/copy
   curl -fsSL -o /root/copy/manifest.json 'MANIFEST-LINK'
   curl -fsSL -o /root/copy/part-0000.bin 'PART-LINK'
   ```

   with one `curl` line for each part. The copy is encrypted and
   authenticated, so the download links are transport only; `restore.sh`
   checks every part against the manifest.
7. **Restore:**

   ```text
   ./dytallix-host/restore.sh /root/copy
   ```

   and type the backup code from paper. It checks the host, checks every
   part's size and SHA-256 against the manifest and joins them into
   `/root/copy/copy.bin` (a part that does not match is named: fetch it
   again), opens the copy, starts the node once to restore it, and waits
   until the node has restored and caught up. It prints the copy's height
   and the height the node reached. On the validator, after the incident decision only, add
   `--accept-history-loss`.

   If it fails: `journalctl -u dytallix-node` shows the supervisor's
   report. The node may hold a partial state, so wipe the host
   (`./dytallix-host/wipe.sh`), install it again from step 5, and retry.
8. **Verify:**
   - `python3 -I -B dytallix-host/host_install.py verify` passes;
   - the restored host's `dytallix_app_height` follows the validator's;
   - on both, `dytallix-operator-rpc --home /var/lib/dytallix/node status`
     shows the same block and application hash at the same height;
   - a restored sentry writes its next snapshot and its backup uploads it
     (`journalctl -u dytallix-backup`), and the status page of a restored
     host serving clients answers.
9. **Delete the copy** from the host (`rm -r /root/copy`); `restore.sh`
   already removed the opened one.
10. **Record** the times (declared, fenced, installed, restored, caught up),
    the copy's height and its age, against the 4-hour objective.

## Monthly and quarterly

- **Monthly:** download the newest complete copy's folder (its manifest and
  every part) to your own drive and, in that folder, check every part:

  ```text
  python3 -c 'import json; [print(p["sha256"] + "  " + p["name"]) for p in json.load(open("manifest.json"))["parts"]]' | shasum -a 256 -c
  ```

  (`sha256sum -c` on Linux). Keep at least one copy per month at the storage
  provider; prune older ones by hand.
- **Quarterly:** the restore drill runs this procedure on staging and
  records its times. CI runs it on every change to the host tooling: H4's
  network job wipes the sentry, reinstalls it, restores it from its copy
  and checks it against the validator (`staging_network.py restore-drill`).

## Not covered

- A replacement on a new address (a new pin plan and bundles).
- Both paper copies of the backup code lost: no copy opens. The sentry
  makes no new copy without a new code, which needs new sealed bundles for
  the sentry.
- No copy younger than 13 days with the sentry lost: the blocks after the
  newest copy are only on the validator, and the sentry cannot verify the
  copy. That is an incident decision.
