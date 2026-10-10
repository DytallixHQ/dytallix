# Monitoring and alerts

The monitor job on each host ([monitoring v1](../architecture/monitoring-v1.md),
M4) pushes a heartbeat every minute and an alert on each change of a rule's
state. Every alert carries `condition`, `severity`, `host`, `value`,
`threshold`, `since` and a link to its section here. The thresholds are the
approved E05 values `monitor.*` (P01, 10 October 2026,
[approval](../../../launch/approvals/P01_E05_ALERT_RULES_2026-10-10.json)).

These are preparation documents. They authorize no live halt, restart, key
operation or state change; the [rules for every incident](README.md#rules-for-every-incident)
apply. The services and accounts (A22) and the backup person are unset.

## Severities

| Severity | Meaning | Response (solo launch profile) |
| --- | --- | --- |
| 1 | The chain, a validator, the keys or monitoring itself is at risk | Paged 24/7, best effort; unacknowledged after 15 minutes it also goes to the backup person |
| 2 | Service is degraded or will be soon | The same day |
| 3 | For the record | The next working day |

## Setting up a host

1. Install the host ([host](host.md)). The installer enables
   `dytallix-monitor.timer`; until the settings exist each run records
   history and alert state and sends nothing, so the alerting service,
   receiving no heartbeat, shows the host as down.
2. Make the host's settings file on your own machine and carry it on your
   own media. Keep it owner-only; it is never committed or published:

   ```json
   {
     "schema": "dytallix.monitor-settings.v1",
     "label": "validator-1",
     "heartbeat_url": "https://…",
     "alert_url": "https://…",
     "escalation_url": null
   }
   ```

   Each host has its own heartbeat check (3 minutes without a heartbeat
   alerts, severity 1). `escalation_url` is null when the alerting service
   escalates by itself; otherwise the job sends a severity 1 alert still
   firing after 15 minutes there, once.
3. At the host console, as root, in the unpacked bundle:
   `./monitor-settings.sh /media/…/validator-1.json`. It checks the label and
   the URLs, installs them root-only (0400) in `/etc/dytallix-monitor/` and
   runs the monitor once. Confirm the heartbeat arrived.

To change a URL, run the same command with a new file; the keys and the
bundle are untouched.

## The staging drill

Every Host install run drills the alerts on the staging sentry (M5,
`staging_network.py monitor-drill`). Its monitor reports to a stand-in
alerting service on its own loopback (`alert_standin.py`, which also alerts
on a heartbeat missing for 3 minutes), with the approved thresholds:

| Drill | Cause | Alerts | Cure |
| --- | --- | --- | --- |
| Restart loop | `systemctl restart dytallix-node` three times, each seen by a run | Restart loop | Resolves when the first start leaves the 15-minute window |
| Full disk | A file on the data disk's file system leaves 3% free | Disk low, Disk critical | The file removed |
| Halt | The validator stopped | Consensus halt | The validator started; blocks resume |
| Stopped monitor | `dytallix-monitor.timer` stopped | Monitoring down (the service) | The timer started; heartbeats resume |

Each alert must arrive firing and then resolved, and the last heartbeat must
show nothing firing. The run's log ends with the evidence (`monitor_drill`:
when each fired and resolved, its value and threshold, every alert seen).
Delivery to the chosen services, acknowledgement and escalation are
exercised on the production hosts (M6).

## Reading the job

- `journalctl -u dytallix-monitor -o cat -n 5`: one JSON line per run with
  `status` (`OK`, or `PARTIAL` with `errors` naming the input it could not
  read), `firing`, `delivered`, `heartbeat` (`SENT`, `FAILED` or
  `NO_SETTINGS`) and `undelivered`.
- `/var/lib/dytallix-monitor/state.json`: each rule's state and since when,
  and alerts waiting for delivery. A failed delivery is retried in order on
  the next run, and the heartbeat carries the count; at most 200 wait.
- `/var/lib/dytallix-monitor/history/`: one line per minute (height, CPU,
  memory, disk, network, peers, mempool, missed blocks, signature failures,
  staked DGT, emitted and burned DRT, restarts, the endpoint's status
  request, firing rules); finished days are compressed, 396 days are kept.
- **History off the host.** Each finished day is encrypted under the
  chain's history code and uploaded to the store, oldest first, one per run;
  `history_pending` in the job's line and the heartbeat says how many wait,
  and an `errors` entry starting `history:` says why one failed. To chart
  it on your own machine, download the copies and open each with the code
  typed from paper:
  `dytallix-root-sign history-open -paper - -in COPY -out DAY.jsonl.gz`.

## Alerts

### Consensus halt

Severity 1, every host: the application height has not changed for 60
seconds. With one validator, every host alerts together. Check that the
validator's `dytallix-node` is active, then follow [halt.md](halt.md). If
only one host alerts, that host is stuck: see its supervisor report.

### Validator outage

Severity 1, in the alerting service: the validator's heartbeat missing for 3
minutes. The host, its network or the monitor job is down. Reach the
console; the node itself is judged by the halt alert of the other hosts.

### Monitoring down

Severity 1, in the alerting service: any host's heartbeat missing for 3
minutes. Check `systemctl status dytallix-monitor.timer` and the job's last
lines; a `FAILED` heartbeat means the host cannot reach the service.

### Missed blocks

Severity 2, validator: 5 or more missed blocks in 10 minutes. Check the
validator's peers (`dytallix_engine_p2p_peers`), clock synchronization and
CPU in the history; see [resource-exhaustion.md](resource-exhaustion.md).

### Disk low

Severity 2, every host: the data disk under 15% free. Grow the disk within
days; the history's `disk_used` gives the growth rate.

### Disk critical

Severity 1, every host: under 5% free. RocksDB stops the node when the disk
fills: follow [resource-exhaustion.md](resource-exhaustion.md) now.

### RPC failing

Severity 2, endpoint: the endpoint's own status request failing or not 200
for 3 minutes (503 means a stale head; see the halt alert). The outside
uptime checker alerts on the same page.

### Supply invariant

Severity 1, validator and sentry: the DRT total differs from genesis plus
emitted minus burned, or DGT issued changed. The DGT check compares with the
first value the job saw and stays firing until it is reset: follow
[supply-mismatch.md](supply-mismatch.md); after the incident is closed,
remove `dgt_issued` from the state file.

### Unexpected mint burn

Severity 1, validator and sentry: in one minute, DRT emitted rose by more
than the per-block ceiling (the approved maximum epoch emission over the
epoch's blocks) times the blocks committed, or fell; or DRT burned rose
while no transaction committed. Follow [supply-mismatch.md](supply-mismatch.md).

### Staking change

Severity 2, validator: staked DGT changed by more than 5% within an hour.
Check the committed bonding, unbonding and penalty transactions; a penalty
also shows as validator evidence.

### Governance event

Severity 3, validator: a governance transaction committed. Record it and
check it against the expected proposals.

### Signature failures

Severity 2, every host: 10 or more invalid signature verifications in 10
minutes. Usually a broken or hostile client; check the endpoint's request
logs and the scheme in `dytallix_app_signature_verifications_total`.

### Restart loop

Severity 1, every host: the node started 3 or more times in 15 minutes. The
unit does not restart itself, so someone or something is starting it. Stop
it, keep the reports, and follow the [restart rule](README.md#rules-for-every-incident).

### Stale metrics

Severity 2, every host: a metric file is older than 60 seconds or missing.
The node or one of its processes stopped writing; check `dytallix-node`.

### Stale backup

Severity 2, sentry: no off-host upload for 26 hours. Check
`journalctl -u dytallix-backup` and the store; see
[disaster-recovery.md](disaster-recovery.md).
