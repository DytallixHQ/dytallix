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
  height. Before its first start that state is raised to the halt height H
  (the signing guard, step 8); that is a floor the key may not sign at or
  below, not a restore. The old validator's last votes for that height may
  still be held by the sentry (or an endpoint, if one is added); step 5
  sets them aside so that no node holds an old vote and a new one for the
  same height and round.
- **Guard before the first start** (incident 2026-11-10). A fresh signing
  state lets the key sign any height. On staging the sentry exited 92 s after
  its start; the replacement, finding no peer and being the only validator,
  started a new chain from genesis and signed heights 1 to about 1,388 with
  the key that had signed 1 to 173,074 on the real chain. With the guard at
  H the key refuses every proposal and vote at or below H, so a replacement
  without a peer stalls at height 1 instead of signing. Never start the
  replacement without it.
- **Start the validator only after the sentry has held** (step 10): its
  engine answering at H from its start until past its startup budget.
  A sentry that is only `active` in systemd may still exit during startup.
- **Stop rule.** Stop the replacement at once (`systemctl stop dytallix-node`
  at its console) if any of these is seen, and do not start it again until
  the cause is understood:
  - its height reads below H after it has once passed H;
  - before it passes H, its block time at its current height is after the
    fence (`dytallix-operator-rpc status`: `latest_block_time`), or it
    shows 0 peers while its height rises: it is making a chain of its own;
  - the sentry serves a height below H (`recovery.py first-block` stops
    with this), or stops answering while the validator runs.

  A height below H is otherwise expected while it catches up: as the only
  validator it does not block-sync, and it follows the sentry from height 1
  through consensus. Its journal then shows `failed signing` with `height
  regression` at each height up to H; that is the guard at work.
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
   In Hetzner Cloud, rebuild protection and delete protection are one
   setting: lifting it for a rebuild also allows a delete, and the reverse.
   Lift it only for the fence, and enable it again (both, the one setting)
   on the server as soon as the rebuild is done; check it shows enabled
   before step 6.
4. **Record the halt height H**, the sentry's `dytallix_app_height`. The
   replacement block syncs to H from the sentry and signs from H+1.
5. **Set the old votes aside.** On the sentry, then on any endpoint:

   ```text
   systemctl stop dytallix-node
   mv /var/lib/dytallix/node/data/cs.wal /var/lib/dytallix/node/data/cs.wal.f17-DATE
   install -d -o dytallix -g dytallix -m 0700 /var/lib/dytallix/node/data/cs.wal
   ```

   Neither node signs, so its consensus log is only a record of messages it
   received; setting it aside drops the old validator's last votes. Do not
   delete it. Leave them stopped.
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
   The bundle must carry `host_install.py guard`
   (`python3 -I -B dytallix-host/host_install.py` prints it in its usage);
   if it does not, stop: rebuild the bundle from a release that has it.
8. **Set the signing guard** at the replacement's console, with H from
   step 4:

   ```text
   python3 -I -B dytallix-host/host_install.py guard H
   ```

   It refuses a running node and a node that has run (one with an engine
   store: wipe it and install it again), and never lowers a state. It prints
   `guarded H`; check the number is H, and on the founder's machine run
   `recovery.py mark guarded --height H`.
9. **Start the sentry** (`systemctl start dytallix-node`). Leave the
   validator stopped.
10. **Hold the sentry** (the start gate). From the founder's machine run
    `recovery.py hold` with the sentry bundle's `service.json`. It reads the
    sentry's engine through its agent until past its startup budget (the
    supervisor's startup, stop and kill bounds plus 60 s; 160 s on staging)
    and prints `GO` only if every reading answered at H. The status page is
    no use here: the sentry serves it only once its node is ready, which
    needs a peer. If the hold fails, the validator stays stopped: find why
    the sentry stopped, start it again, and hold again. A height above H
    means something else is signing: stop, and treat it as
    [key compromise](key-compromise.md).
11. **Start the validator** after `GO`, then any endpoint. The validator
    catches up from the sentry to H, then signs from H+1. Apply the stop
    rule while it catches up.
12. **Verify:**
   - on the validator, `dytallix_app_height` passes H and keeps rising, and
     `python3 -I -B dytallix-host/host_install.py verify` passes;
   - the sentry's height (and any endpoint's) follows;
   - the status view over the sentry's client channel lists the same
     validator set as before H.
13. **Record** the times (declared, fenced, installed, guarded, sentry
    held, first block after H),
    H and the bundle's SHA-256, against the 8-hour objective. The quarterly
    restore drill runs this procedure on staging and records the same; the
    CI `validator-recovery-drill` (simulation plan, T9; `recovery.py`) runs it
    on the staging network. On real hosts the founder's machine records the
    drill as it goes, every instant on its clock: `recovery.py start` at
    step 1 (it reads the validator set first), `recovery.py mark` as each
    console step is done (`fenced`, `wal_set_aside`, `installed`,
    `guarded --height H`, `sentry_started`, `validator_started`),
    `recovery.py halt` for step 4, `recovery.py hold` for step 10,
    `recovery.py first-block` after step 11, and `recovery.py finish` for
    step 12, once the replacement's staging agent is installed again. The
    evidence fails without the guard at H or the hold before the validator
    started. Keep the machine awake and online from the declaration to the
    first block.

## Open items

- The sentry's startup budget and restart policy: on 2026-11-10 its
  supervisor exited at +92 s (`BOUNDED_STARTUP_FAILURE`, `OTHER_FAILURE`)
  and systemd did not restart it. Under separate investigation; this
  procedure does not change them. Until it is fixed the hold may fail and
  hold the validator back, which is the safe outcome.
- Why the validator and sentry did not peer, and whether catching up a long
  chain through consensus (the only validator does not block-sync) fits the
  6-hour catch-up objective.
- Bundles sealed before the guard carry a `host_install.py` without it
  (step 7 stops on them).
- The staging replacement from 2026-11-10 holds the forked chain in its
  stores: before any retry it is wiped and installed again, never guarded
  and restarted (the guard refuses it). A retry needs a new approval; the
  2026-11-06 one does not authorize a second run.
- The hold budget is the sentry's startup, stop and kill bounds plus 60 s,
  not its 6-hour catch-up budget, inside which it may still exit. Nothing
  yet watches the sentry stay up after the validator starts beyond
  `recovery.py first-block` polling its status page.

## Not covered

- A replacement on a new address (a new pin plan and bundles).
- A lost seal code with both copies gone, or a stolen one: follow
  [key compromise](key-compromise.md).
- Losing the sentry as well: [disaster recovery](disaster-recovery.md)
  (F19), after the incident decision.
