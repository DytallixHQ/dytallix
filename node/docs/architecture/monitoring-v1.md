# Monitoring and alerts v1 (G27, G28)

Engineering task E05, launch gates G27 (monitoring) and G28 (alerting).
[Node metrics](metrics-v1.md) made the signals exist. This design collects
them, adds the host's own, and turns them into alerts that reach a person.
Targets and severities are the
[operations objectives](../../../launch/operations/OBJECTIVES.md)'.

## Problem

- **Nothing collects the metrics.** The node writes `dytallix-app.prom` and
  `dytallix-engine.prom` every 15 seconds, but no host runs a collector.
  Inbound traffic is dropped and the Prometheus listener is refused, so
  nothing can scrape the hosts either.
- **Measurements are missing.** Gate G27's list (Day 9) needs these, which
  nothing records today:
  - transaction throughput and failures, gas, fees and rewards;
  - governance and slashing events, and signature verification failures;
  - PQC signing and verification time, and node restarts;
  - the host's CPU, memory, disk and network.
- **Nothing alerts.** No alert condition is evaluated, no one is paged, and a
  silent monitor looks the same as a healthy chain.
- **The independent monitor can't see a halt.** The endpoint's status page
  answers 200 with the last height even when the chain has stopped.

## Decisions (P01, 9 October 2026)

[Approval](../../../launch/approvals/P01_E05_MONITORING_2026-10-09.json):

1. **Push from each host.** A monitor job on each host, run every minute by
   a timer like the sentry's backup job, reads the node's metric files and
   the host's own counters, evaluates the alert rules, and pushes a
   heartbeat and any alert outbound with the host's `curl`. Nothing listens
   and no host is added. This is a deliberate exception to "nothing else
   runs on the host" ([host setup v1](host-setup-v1.md)).
2. **A halt shows on the status page.** The endpoint's status page answers
   503 when the newest block is older than an approved age, so any uptime
   checker detects a halt or a stuck endpoint from the HTTP status alone.
3. **The founder plus one backup.** The founder is the primary on-call. A
   severity 1 alert not acknowledged within 15 minutes also goes to one
   backup person, a root key holder named outside this repository.
4. **Vendor-neutral delivery.** Heartbeats and alerts go to webhook URLs held
   in each host's settings. The founder chooses the free services and
   creates their accounts later (artifact A22).

## Decisions (P01, 10 October 2026)

[Approval](../../../launch/approvals/P01_E05_ALERT_RULES_2026-10-10.json):

5. **The alert rules as proposed.** The fourteen rules and severities below,
   escalation after 15 minutes and 396 days of history are E05 values
   `monitor.*`.
6. **History off the host, under its own code.** Each host uploads its
   finished daily history file, encrypted. The validator and endpoint do
   not get the chain's backup code: a separate history code (its own paper
   line) and write-only upload key are sealed on every host (M4b).
7. **Replaceable settings.** The webhook URLs are not sealed with the node
   keys, so changing a service never touches them: the installer's
   `monitor-settings` command installs them from the founder's own media,
   root-only, and they never enter the bundle. This replaces "sealed
   settings" in decision 4.

## Rule

### The monitor job

- **Where.** `dytallix-monitor.timer` (every minute, on the minute) and the
  oneshot `dytallix-monitor.service` on every host, in the host bundle. They
  run `python3 -I -B monitor.py --config monitor.json` from the release as
  root with an empty capability set, `NoNewPrivileges` and a read-only
  system; it can write only `/var/lib/dytallix-monitor`. Root without
  capabilities, like the backup job, so it reads its root-only settings and
  the backup job's state without changing their modes; it reads
  `/var/lib/dytallix/metrics` (world-readable), `/proc` and the node's unit
  state (`systemctl show`).
- **Reads, each minute:**
  - both metric files and their write times;
  - CPU (`/proc/stat`), memory (`/proc/meminfo`), the data disk's free space
    and its growth (`statvfs`), and network bytes (`/proc/net/dev`);
  - `dytallix-node`'s state and restart count (`systemctl show`);
  - on the sentry, the backup job's last upload.
- **State.** `/var/lib/dytallix-monitor` keeps the previous sample for rates,
  each alert's state (firing or resolved, and since when), and the metric
  history (see Retention).
- **Settings.** `/etc/dytallix-monitor/webhooks.json`
  (`dytallix.monitor-settings.v1`: the host's label, `heartbeat_url`,
  `alert_url` and `escalation_url` or null), root 0400, installed and
  replaced by `monitor-settings.sh FILE`. Each URL is https (plain http only
  to a stand-in on the host). Without them the job still records history
  and alert state and sends nothing; the service, missing the heartbeat,
  alerts.
- **Sends**, outbound only, through the host's `curl`, each URL and payload
  passed in curl's configuration on its standard input, never on a command
  line. TLS stays in `curl` and out of every release binary, as the backup
  job's upload already does (G35):
  - a heartbeat every run to the host's heartbeat URL. The service alerts
    when it stops, which covers a host down, a monitor down and failed
    collection;
  - each alert on a change of state (firing or resolved) to the alert URL,
    as JSON: severity, condition, host, value, threshold and a runbook link.
- **Escalation.** The alerting service holds acknowledgements. Where it can
  escalate, it does so after 15 minutes. Otherwise the job sends a
  severity 1 alert still firing after 15 minutes to the backup URL too.
- **Failed delivery.** The job retries on its next run and counts failures;
  the heartbeat carries the count. A service that stops receiving
  heartbeats alerts on its own.

### The status page

The endpoint's `/status` answers 503, with the same JSON body, when the
engine's newest block time is more than `status_max_head_age_seconds` old:
60 seconds (P01, 10 October 2026,
[approval](../../../launch/approvals/P01_E05_STATUS_HEAD_AGE_2026-10-10.json)),
about 12 missed blocks. An unreadable block time also answers 503, and a
block time ahead of the endpoint's clock counts as fresh. The independent
uptime checker polls it each minute. That one check measures the chain's
and the endpoint's service targets from outside the hosts.

Built in M2: the adapter's `--status-max-head-age-seconds`, the
supervisor's `adapter_status_max_head_age_seconds` (required with the
status page), and the host files' value from `E05_VALUES.json`.

### Measurements (G27, Day 9)

Built in M3 (P01, 10 October 2026,
[approval](../../../launch/approvals/P01_E05_METRIC_SET_2026-10-10.json);
[metrics v1](metrics-v1.md), decision 3): the application's transaction,
gas, evidence, root control and signature verification measurements and the
engine's signing time. Restarts, host resources and RPC latency come with
the monitor job (M4a).

| Measurement | Source |
| --- | --- |
| Block height, production, time; consensus rounds | Engine: height, block interval, rounds; app: height |
| Validator participation, missed blocks, voting power, availability | Engine: validators, missing validators, missed blocks, last signed height |
| Peer count | Engine: `p2p_peers` |
| Mempool size | Engine: mempool size and bytes |
| Transaction throughput and failure rate, gas, fees | App: `transactions_total{kind, result}`, `gas_used_total`; fees burned (`supply_udrt{bucket="burned"}`) |
| Reward distribution, DRT issuance, DGT supply, burns | App: supply buckets (emitted, the reward pools, burned, DGT issued) |
| Delegation activity, slashing events, governance events | App: staked, bonding and unbonding buckets; `validator_evidence_total` and the penalty reserve; `transactions_total{kind="governance"}` and `root_controls_total{kind}` |
| Signature verification failures; PQC signing and verification time | App: `signature_verifications_total{scheme, result}`, `signature_verification_seconds{scheme}`; engine: `privval_sign_seconds` |
| Node restarts | Monitor: `systemctl` restart count |
| CPU, memory, disk, disk growth, network traffic | Monitor: `/proc`, `statvfs` |
| RPC latency and error rate | Monitor on the endpoint: a local timed status request; uptime checker from outside |

### Alerts (G28, Day 9)

Approved as proposed (P01, 10 October 2026; E05 values `monitor.*`). What to
do for each is in the [monitoring runbook](../operations/monitoring.md).

| Alert | Rule (proposed) | Severity | Where |
| --- | --- | --- | --- |
| Consensus halt | App height unchanged for 60 s; status page 503 | 1 | All hosts; uptime checker |
| Validator outage | The validator's heartbeat missing for 3 minutes | 1 | Alerting service |
| Abnormal missed blocks | 5 or more missed in 10 minutes | 2 | Validator |
| Disk exhaustion | Data disk under 15% free (2), under 5% (1) | 2, 1 | All hosts |
| RPC outage | Status page not 200 for 3 minutes; local status request failing | 2 | Uptime checker; endpoint |
| Supply invariant violation | DRT total differs from genesis plus emitted minus burned; DGT issued changes | 1 | Validator, sentry |
| Unexpected mint or burn | Emitted beyond the issuance schedule; burned without fees | 1 | Validator, sentry |
| Abnormal staking event | Staked DGT changes by more than 5% in an hour | 2 | Validator |
| Abnormal governance event | Any governance proposal or execution event | 3 | Validator |
| Repeated signature verification failures | 10 or more in 10 minutes | 2 | All hosts |
| Node restart loop | 3 or more restarts in 15 minutes | 1 | All hosts |
| Stale metrics | A metric file older than 60 s | 2 | All hosts |
| Stale backup | No upload for 26 hours | 2 | Sentry |
| Monitoring down | Any host's heartbeat missing for 3 minutes | 1 | Alerting service |

A divergence between the validator and the sentry stops the sentry, which
the halt and stale-metric alerts catch on the sentry.

How the job judges them (built in M4a):
- **Halt:** the time since `dytallix_app_height` last changed.
- **Windows** (missed blocks, signature failures, restarts): the rise of the
  counter since the oldest sample in the window; a counter that went back
  (a process restart) counts from zero.
- **Supply:** DRT `total` against `genesis + emitted - burned` each run; DGT
  `issued` against the first value the job saw, which stays firing until an
  operator resets it. Emission beyond the schedule is a rise of `emitted`
  above the ceiling per block (the approved `max_udrt` over `epoch_blocks`,
  rounded up: 1,000,000,000 uDRT) times the blocks committed; a burn without
  fees is a rise of `burned` while no transaction committed, judged only
  within one node process.
- **Staking:** staked DGT now against the sample an hour ago.
- **Restarts:** a new `InvocationID` of `dytallix-node.service`; the unit
  never restarts itself.
- **Validator outage, monitoring down:** the alerting service's heartbeat
  checks, one per host.

### Retention

- **Metrics, 13 months.** Each host's job appends one compact line per
  minute to a daily file and compresses the previous day's. It keeps 13
  months, about 30 MB per host.
- **Logs, 90 days.** journald already keeps 90 days.
- **Alert records, permanently.** Alert records are kept by the alerting
  service, plus the founder's monthly export.
- **Off the host (decision 6, built in M4b):** each host also uploads its
  finished daily file, encrypted under the history code, to the backup
  store, so the founder can draw dashboards on their own machine from the
  history. The hosts are console-only.
  - **Code and key.** The key step makes the chain's history code once
    (`dytallix-root-sign history-code`; paper line `dytallix-history-CHAIN`,
    typed back) and seals it, with a separate write-only upload key
    (`--history-upload`, refused if it is the backup key), with every host's
    keys. The installer places them in `/etc/dytallix-monitor/` (root 0400).
  - **Format** (`dytallix.history.v1`): the backup copy's AES-256-GCM chunk
    stream under its own header (chain, host, UTC day, salt) and key domain,
    so a copy cannot pass for another host's or day's, and neither a backup
    nor a history copy opens as the other.
  - **Upload.** After the heartbeat, each run encrypts the oldest finished
    day not yet uploaded (`history-seal`) and uploads it with curl (AWS
    Signature V4, the key on curl's standard input) as
    `PREFIX/CHAIN/history/HOST/DAY-SHA256.bin`; it records the day only once
    the store accepts it. The heartbeat carries `history_pending` and
    `history_failures`.
  - **Opening.** On the founder's machine, `dytallix-root-sign history-open
    -paper - -in COPY -out DAY.jsonl.gz` with the code typed from paper.

## Steps

| Step | Content | Output change |
| --- | --- | --- |
| M1 | This design and the decisions | None |
| M2 | The status page's 503 on a stale head; `status_max_head_age` | Status page |
| M3 | The missing application and engine measurements | New metrics |
| M4a | The monitor job, its timer, the settings command, alert rules, escalation and local history; the threshold values | Host bundle |
| M4b | The history code and upload key, `dytallix-root-sign` history commands, the daily encrypted upload | Host bundle, key step |
| M5 | Staging drill in CI: a stand-in alert receiver; a halt, a restart loop, a full disk and a stopped monitor each alert and resolve | Evidence |
| M6 | On the production hosts: the services chosen, each alert exercised, delivery, acknowledgement and escalation recorded (T-phase) | Acceptance evidence |

## What does not change

The node's metric files and their format, the firewall (inbound stays
closed), and every release binary's freedom from TLS (G35).
