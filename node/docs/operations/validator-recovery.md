# Validator recovery (F17)

The validator's host is lost: destroyed, unreachable for good, or its disk
failed. Under the solo launch profile there is one validator, so the chain
is halted until a replacement signs. The approved recovery time is 8 hours:
about 2 hours to provision and up to 6 hours of catch-up
([operations objectives](../../../launch/operations/OBJECTIVES.md)).
Severity 1.

## Rules

- **Fence first** (P01, 6 October 2026,
  [approval](../../../launch/approvals/P01_E05_HOST_RECOVERY_2026-10-06.json)).
  The old validator server is powered off and deleted, or reinstalled, in
  the provider console, and seen gone, before the replacement is installed.
  There is one validator key. Two running copies sign twice, and the first
  double sign removes the validator for good
  ([penalties v1](../architecture/penalties-v1.md)); with one validator that
  ends the chain.
- **Never restore signing state from a backup.** The replacement starts from
  the sealed bundle's fresh signing state and signs again from the next
  height. The old validator's last votes for that height may still be held
  by the sentry or the endpoint; step 5 sets them aside so that no node holds
  an old vote and a new one for the same height and round.
- **If in doubt, wait.** Waiting costs time against the 8 hours; a second
  signer can end the chain.

## You need

- Provider console access.
- The validator's bundle for the release the chain runs (the latest one
  staged and switched to, or the install bundle if there has been no
  upgrade), and its SHA-256 from paper or the release.
- One of the two paper copies of the validator's seal code. Without either
  copy the sealed keys cannot be opened, the validator key is lost, and with
  one validator the chain cannot continue.

## Steps

1. **Declare** the incident and note the time.
2. **Confirm** the validator is lost: the provider console shows it down or
   gone, and on the sentry the height has stopped
   (`grep '^dytallix_app_height' /var/lib/dytallix/metrics/dytallix-app.prom`
   twice, a minute apart).
3. **Fence.** In the provider console, power the old validator server off
   and delete it (or reinstall its operating system). Wait until the
   console shows it deleted or reinstalled. Do not continue until then.
4. **Record the halt height H**, the sentry's `dytallix_app_height`. The
   replacement block syncs to H from the sentry and signs from H+1.
5. **Set the old votes aside.** On the sentry, then on the endpoint:

   ```text
   systemctl stop dytallix-node
   mv /var/lib/dytallix/node/data/cs.wal /var/lib/dytallix/node/data/cs.wal.f17-DATE
   install -d -o dytallix -g dytallix -m 0700 /var/lib/dytallix/node/data/cs.wal
   ```

   Neither node signs, so its consensus log is only a record of messages it
   received; setting it aside drops the old validator's last votes. Do not
   delete it. Leave both nodes stopped.
6. **Provision** the replacement: Ubuntu 24.04 with the host base setup
   ([host setup v1](../architecture/host-setup-v1.md#host-packages-and-settings)),
   and the old server's public IPv4 address, reassigned to it in the
   provider console. The pin plan, the sentry's pins and firewall and the
   validator's bundle all name that address. If it cannot be reused, stop:
   a new address needs a new pin plan and new bundles for the validator and
   the sentry, which this procedure does not cover.
7. **Install** at the replacement's console as in host setup v1, step 5:
   download the bundle, check its SHA-256, unpack it, run
   `./dytallix-host/install.sh` and type the seal code. Do not start it yet.
8. **Start in order:** the sentry (`systemctl start dytallix-node`), then
   the replacement validator, then the endpoint. The validator block syncs
   from the sentry to H, then signs from H+1.
9. **Verify:**
   - on the validator, `dytallix_app_height` passes H and keeps rising, and
     `python3 -I -B dytallix-host/host_install.py verify` passes;
   - the sentry's and the endpoint's heights follow;
   - the endpoint's status view lists the same validator set as before H.
10. **Record** the times (declared, fenced, installed, first block after H),
    H and the bundle's SHA-256, against the 8-hour objective. The quarterly
    restore drill runs this procedure on staging and records the same.

## Not covered

- A replacement on a new address (a new pin plan and bundles).
- A lost seal code with both copies gone, or a stolen one: follow
  [key compromise](key-compromise.md).
- Losing the sentry as well: [disaster recovery](disaster-recovery.md)
  (F19), after the incident decision.
